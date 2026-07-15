#![deny(missing_docs)]
//! [`NoOpObserver`] — installed in TLS for filtered requests so emit
//! sites still call `current_observer()` and emit through the trait
//! without paying any downstream attribution cost.

use std::sync::Arc;

use crate::observer::{install_observer, AuditObserver, ObserverGuard};

/// Trivial observer that drops every event. Used when
/// [`crate::config::AuditConsumerFilter`] excludes a request's kind
/// — emit sites still go through the observer call but nothing
/// downstream happens.
#[derive(Debug, Default)]
pub struct NoOpObserver;

impl AuditObserver for NoOpObserver {
    // All methods inherit the trait default no-ops.
}

/// Guard returned by [`install_noop_observer`]. Newtype wrapper around
/// [`ObserverGuard`] so callers can mention the type by name in API
/// signatures.
pub struct NoOpObserverGuard(#[allow(dead_code)] ObserverGuard);

/// Install [`NoOpObserver`] on the calling thread's TLS slot,
/// returning an RAII guard that restores the previous observer on
/// drop.
///
/// The session-side public audited entry-point invokes this when
/// [`crate::AuditConfig`]'s consumer filter rejects the request's
/// kind. The TLS slot stays populated so emit sites that look up
/// `current_observer()` see `Some(...)` (no behavioural difference
/// from active requests at the call-site level — only the observer's
/// methods are no-ops).
#[must_use]
pub fn install_noop_observer() -> NoOpObserverGuard {
    NoOpObserverGuard(install_observer(Arc::new(NoOpObserver)))
}
