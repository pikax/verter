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
//! 2. A [`crate::request_context::RequestContext`] keyed by
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
use verter_audit::{
    LspRequestPayload, RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit,
    RequestStoreAudit, RequestTimingAudit,
};

use crate::host_audit_runtime::AuditRequestRegistration;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::VerterHost;

/// Session object held by an audited LSP handler for the duration
/// of one request. Constructed by [`VerterHost::lsp_audit_begin`];
/// finalised exactly once by either [`Self::finalize_ok`] or
/// [`Self::finalize_cancelled`].
///
/// Internally an `Active(...)` session owns the
/// [`AuditRequestRegistration`] and its [`RequestContextGuard`]; the
/// guard is dropped after `finalize` so the per-request counters
/// remain coherent with the published record.
///
/// `Noop` sessions hold no state. Their finalize methods return
/// `None` without entering the records store and without locking
/// the registry.
pub enum LspAuditSession {
    /// Active session — `audit_enabled = true` and the consumer
    /// filter accepted [`RequestKind::Lsp`].
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
pub struct ActiveLspSession {
    request_id: u64,
    method: LspMethodTag,
    canonical_id: String,
    registration: Arc<AuditRequestRegistration>,
    /// TLS guard for `current_observer()`. Held until finalise so
    /// per-request counters stay coherent with the record we
    /// publish; dropped inside `finalize_*` AFTER the
    /// `registration.finalize` call.
    tls_guard: Option<RequestContextGuard>,
}

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
        match self {
            Self::Noop => None,
            Self::Active(mut active) => {
                let record = build_record(active.request_id, &active.canonical_id, payload);
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
    /// session's method / canonical id; sibling fields stay at
    /// their `Default` values.
    ///
    /// Idempotent — the underlying registration ignores subsequent
    /// calls. Returns `Some(record)` only on the first finalize
    /// against an `Active` session.
    pub fn finalize_cancelled(self) -> Option<RequestAuditRecord> {
        match self {
            Self::Noop => None,
            Self::Active(mut active) => {
                let payload = LspRequestPayload {
                    method: active.method.clone(),
                    error: Some("cancelled".to_string()),
                    ..LspRequestPayload::default()
                };
                let record = build_record(active.request_id, &active.canonical_id, payload);
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

fn build_record(
    request_id: u64,
    canonical_id: &str,
    payload: LspRequestPayload,
) -> RequestAuditRecord {
    RequestAuditRecord {
        request_id,
        canonical_id: canonical_id.to_string(),
        kind: RequestKind::Lsp {
            method: payload.method.clone(),
        },
        parent_request_id: None,
        from_cache: false,
        timings: RequestTimingAudit::default(),
        memory: RequestMemoryAudit::default(),
        store: RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: RequestKindPayload::Lsp(payload),
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
    /// `canonical_id` is the canonical / virtual id of the file the
    /// request operates on (e.g. the LSP URI's resolved canonical).
    /// Passing the empty string is acceptable for handlers that
    /// operate workspace-wide.
    #[must_use]
    pub fn lsp_audit_begin(
        self: &Arc<Self>,
        method: LspMethodTag,
        canonical_id: &str,
    ) -> LspAuditSession {
        if !self.config.audit_enabled {
            return LspAuditSession::Noop;
        }

        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let footprint_capture = self.config.footprint_capture;
        let timing_capture = self.config.audit_timing_capture;
        let ctx = RequestContext::with_kind_and_timing(
            request_id,
            Arc::<str>::from(canonical_id),
            RequestKind::Lsp {
                method: method.clone(),
            },
            footprint_capture,
            timing_capture,
            None,
        );

        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));

        // The consumer filter may reject the kind. In that case
        // `AuditRequestRegistration::new` returned `Noop` and we
        // skip the TLS guard entirely — emitting through
        // `current_observer()` would still hit the per-context
        // counters, but the registration has no record to publish.
        match registration.as_ref() {
            AuditRequestRegistration::Noop => LspAuditSession::Noop,
            AuditRequestRegistration::Active(_) => {
                let _ = ctx.install_audit_registration(Arc::clone(&registration));
                let tls_guard = RequestContextGuard::install(ctx);
                LspAuditSession::Active(ActiveLspSession {
                    request_id,
                    method,
                    canonical_id: canonical_id.to_string(),
                    registration,
                    tls_guard: Some(tls_guard),
                })
            }
        }
    }
}
