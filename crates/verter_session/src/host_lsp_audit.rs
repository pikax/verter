#![deny(missing_docs)]
//! `VerterHost::lsp_audit_begin` — LSP-side audited request session.
//!
//! Wraps the [`crate::host_audit_runtime::AuditRequestRegistration`]
//! lifecycle in a small session object so each LSP handler can drive
//! `*_with_audit` work without reaching across the
//! `register_active_request` / `finalize_active_request` privacy
//! boundary on [`crate::host_audit_runtime::HostAuditRuntime`].
//!
//! The session encapsulates:
//!
//! 1. A fresh request id (stamped via the host's monotonic counter).
//! 2. A [`verter_type_engine::request_context::RequestContext`] keyed by
//!    [`verter_audit::RequestKind::Lsp`] with a producer-supplied
//!    [`verter_audit::payloads::tags::LspMethodTag`].
//! 3. An [`crate::host_audit_runtime::AuditRequestRegistration`]
//!    constructed BEFORE any TLS guard so the active-request
//!    registry sees the slot for the duration of the handler call.
//! 4. Helpers to finalize either with a normal payload
//!    ([`LspAuditSession::finalize_ok`]) or with the cancellation
//!    marker required by the LSP cancellation contract
//!    ([`LspAuditSession::finalize_cancelled`]).
//!
//! Both finalize paths are idempotent through the underlying
//! [`crate::host_audit_runtime::AuditRequestRegistration::finalize`];
//! the session's `Drop` impl is purely defensive (the registration's
//! own `Drop` removes the entry from the active-request registry on
//! panic / unwind paths).
//!
//! When `audit_enabled` is `false` on [`crate::HostConfig`] or the
//! consumer filter rejects the kind, the session is `Noop`: every
//! method short-circuits and the handler runs without observability
//! cost. Producers always go through [`Self::begin`] regardless;
//! the gating decision is centralised here.

use std::sync::Arc;

use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{LspRequestPayload, RequestAuditRecord, RequestTargetIdentity};
#[cfg(feature = "semantic-observe")]
use verter_audit::{
    RequestKind, RequestKindPayload, RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit,
};

#[cfg(feature = "semantic-observe")]
use crate::host_audit_runtime::AuditRequestRegistration;
use crate::VerterHost;
#[cfg(feature = "semantic-observe")]
use verter_type_engine::request_context::RequestContext;

/// Session object held by an audited LSP handler for the duration
/// of one request. Constructed by [`VerterHost::lsp_audit_begin`];
/// finalised exactly once by either [`Self::finalize_ok`] or
/// [`Self::finalize_cancelled`].
///
/// Internally an `Active(...)` session owns the
/// [`AuditRequestRegistration`] and the audit-observer slot guard; the
/// guard is dropped after `finalize`.
///
/// Neither arm installs a governing request context for the handler: the
/// handler is an async window that may resume on any runtime worker, and
/// the requests it issues install their own contexts exactly as they do
/// without capture, so budget, cancellation and partiality never depend
/// on whether the LSP request is captured.
///
/// `Noop` sessions hold no state. Their finalize methods return
/// `None` without entering the records store and without locking
/// the registry.
pub enum LspAuditSession {
    /// Active session — `audit_enabled = true` and the consumer
    /// filter accepted [`RequestKind::Lsp`]. Exists only with
    /// `semantic-observe`.
    #[cfg(feature = "semantic-observe")]
    Active(ActiveLspSession),
    /// No-op session — audit disabled or kind filtered out. All
    /// finalize methods short-circuit to `None`.
    Noop,
}

/// Active arm of [`LspAuditSession`]. Owns the registration, the
/// request id, and the TLS guard for the request's lifetime.
///
/// Access the request id via [`Self::request_id`] when constructing
/// payloads or asserting lifecycle membership.
#[cfg(feature = "semantic-observe")]
pub struct ActiveLspSession {
    request_id: u64,
    method: LspMethodTag,
    target_identity: RequestTargetIdentity,
    /// Parent-request id captured off the `RequestContext` at
    /// `lsp_audit_begin` (sniffed from the scheduler TLS). Populates
    /// the record's `parent_request_id` so a sub-request LSP handler
    /// carries parent correlation.
    parent_request_id: Option<u64>,
    registration: Arc<AuditRequestRegistration>,
    /// Observation-only guard for `current_observer()`: plants the
    /// session's captured context in the audit observer slot, never in the
    /// request-context slot. Held until finalise; dropped inside
    /// `finalize_*` AFTER the `registration.finalize` call.
    tls_guard: Option<verter_audit::observer::ObserverGuard>,
}

#[cfg(feature = "semantic-observe")]
impl ActiveLspSession {
    /// The freshly-stamped request id for this session.
    #[must_use]
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Borrow the LSP method tag this session was opened for. Useful
    /// when a single handler dispatches across several method tags
    /// (e.g. goto-definition vs type-definition).
    #[must_use]
    pub fn method(&self) -> &LspMethodTag {
        &self.method
    }
}

impl LspAuditSession {
    /// `Some(request_id)` for active sessions; `None` for noops.
    #[must_use]
    pub fn request_id(&self) -> Option<u64> {
        match self {
            Self::Noop => None,
            #[cfg(feature = "semantic-observe")]
            Self::Active(active) => Some(active.request_id),
        }
    }

    /// Finalise with a populated [`LspRequestPayload`]. Idempotent —
    /// the underlying registration ignores subsequent calls.
    /// Returns `Some(RequestAuditRecord)` when the session was
    /// `Active` AND the registration's first finalize call wins;
    /// `None` otherwise (noop session, or losing a race with the
    /// defensive drop path).
    pub fn finalize_ok(self, payload: LspRequestPayload) -> Option<RequestAuditRecord> {
        #[cfg(not(feature = "semantic-observe"))]
        let _ = payload;
        match self {
            Self::Noop => None,
            #[cfg(feature = "semantic-observe")]
            Self::Active(mut active) => {
                let record = build_record(
                    active.request_id,
                    active.target_identity,
                    active.parent_request_id,
                    payload,
                );
                let won = active.registration.finalize(record.clone());
                // Drop the TLS guard AFTER finalize so per-request
                // counters stay coherent with the record we publish.
                active.tls_guard.take();
                if won {
                    Some(record)
                } else {
                    None
                }
            }
        }
    }

    /// Finalise with the cancellation marker required by the LSP
    /// cancellation contract. The produced payload carries
    /// `error: Some("cancelled".to_string())` and matches the
    /// session's method / target identity; sibling fields stay at
    /// their `Default` values.
    ///
    /// Idempotent — the underlying registration ignores subsequent
    /// calls. Returns `Some(record)` only on the first finalize
    /// against an `Active` session.
    pub fn finalize_cancelled(self) -> Option<RequestAuditRecord> {
        match self {
            Self::Noop => None,
            #[cfg(feature = "semantic-observe")]
            Self::Active(mut active) => {
                let payload = LspRequestPayload {
                    method: active.method.clone(),
                    error: Some("cancelled".to_string()),
                    ..LspRequestPayload::default()
                };
                let record = build_record(
                    active.request_id,
                    active.target_identity,
                    active.parent_request_id,
                    payload,
                );
                let won = active.registration.finalize(record.clone());
                active.tls_guard.take();
                if won {
                    Some(record)
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(feature = "semantic-observe")]
fn build_record(
    request_id: u64,
    target_identity: RequestTargetIdentity,
    parent_request_id: Option<u64>,
    payload: LspRequestPayload,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: target_identity.legacy_canonical_id().to_string(),
        target_identity: Some(target_identity),
        kind: RequestKind::Lsp {
            method: payload.method.clone(),
        },
        parent_request_id: parent_request_id.map(|id| id.to_string()),
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::Lsp(payload),
        capture_state: verter_audit::AuditCaptureState::ActiveStored,
        trace_id: String::new(),
    }
}

impl VerterHost {
    /// Open an audited LSP handler session.
    ///
    /// Stamps a fresh request id, constructs a per-request context
    /// keyed by [`RequestKind::Lsp { method }`], builds the
    /// [`AuditRequestRegistration`] (which inserts into the
    /// active-request registry on the `Active` arm), installs the
    /// TLS observer guard, and returns the session for the handler
    /// to drive.
    ///
    /// When `audit_enabled = false` or the consumer filter rejects
    /// the kind, the returned session is [`LspAuditSession::Noop`]
    /// and all finalize methods short-circuit. Callers always go
    /// through this entry-point so the gating decision is
    /// centralised; the `Noop` path bypasses both the active-request
    /// registry insertion and the records store.
    ///
    /// `target_identity` is the exact registered canonical / virtual id, the
    /// raw request URI when registration has not happened yet, or
    /// [`RequestTargetIdentity::NotApplicable`] for a genuinely
    /// workspace-wide operation.
    #[must_use]
    pub fn lsp_audit_begin(
        self: &Arc<Self>,
        method: LspMethodTag,
        target_identity: RequestTargetIdentity,
    ) -> LspAuditSession {
        #[cfg(feature = "semantic-observe")]
        if self.config.audit_enabled {
            return self.lsp_audit_begin_captured(method, target_identity);
        }
        #[cfg(not(feature = "semantic-observe"))]
        let _ = (method, target_identity);
        LspAuditSession::Noop
    }

    #[cfg(feature = "semantic-observe")]
    fn lsp_audit_begin_captured(
        self: &Arc<Self>,
        method: LspMethodTag,
        target_identity: RequestTargetIdentity,
    ) -> LspAuditSession {
        let canonical_id = target_identity.legacy_canonical_id();
        let request_id = self.next_request_id();
        #[cfg(feature = "semantic-observe")]
        verter_type_engine::request_context::increment_requests_created();

        let ctx = RequestContext::for_root_entry(
            request_id,
            Arc::<str>::from(canonical_id),
            || RequestKind::Lsp {
                method: method.clone(),
            },
            verter_audit::observe::ObserveMode::Captured,
            self.config.footprint_capture,
            self.config.audit_timing_capture,
            None,
            self.config.projection_op_budget,
            verter_execution::cancellation::CancellationToken::new(),
        );

        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));

        // The consumer filter may reject the kind. In that case
        // `AuditRequestRegistration::new` returned `Noop`: the session
        // is uncaptured and plants no observer.
        match registration.as_ref() {
            AuditRequestRegistration::Noop => LspAuditSession::Noop,
            AuditRequestRegistration::Active(_) => {
                let _ = ctx.install_audit_registration(registration.clone());
                let parent_request_id = ctx.parent_request_id;
                let tls_guard = verter_audit::observer::install_observer(
                    ctx as Arc<dyn verter_audit::AuditObserver>,
                );
                LspAuditSession::Active(ActiveLspSession {
                    request_id,
                    method,
                    target_identity,
                    parent_request_id,
                    registration,
                    tls_guard: Some(tls_guard),
                })
            }
        }
    }
}
