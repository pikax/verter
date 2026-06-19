//! Discriminating test for the [`verter_audit::current_observer`] TLS
//! plumbing in [`verter_session::request_context::RequestContextGuard`].
//!
//! Pre-change tree (no `verter_audit::install_observer` call inside
//! `RequestContextGuard::install`): `current_observer()` returns
//! `None` while a session-side `RequestContext` is installed in TLS.
//! Post-change tree: `current_observer()` returns the same
//! `Arc<RequestContext>` as `current_request_context()`, and event
//! emission through the substrate observer reaches the same
//! per-request counters as the existing typed accessor.

use std::sync::Arc;

use verter_audit::{current_observer, AuditEvent};
use verter_session::request_context::{
    current_request_context, RequestContext, RequestContextGuard,
};

#[test]
fn substrate_current_observer_returns_session_request_context_when_installed() {
    // Pre-state: no observer is installed on this thread.
    assert!(
        current_observer().is_none(),
        "pre-state: substrate observer slot must be empty before any guard installs"
    );

    let ctx = RequestContext::new(42, Arc::from("/probe.vue"), false, None);
    let _guard = RequestContextGuard::install(Arc::clone(&ctx));

    // The session-side context accessor returns our context.
    let session_view = current_request_context().expect("session ctx must be installed");
    assert_eq!(session_view.request_id, 42);

    // The substrate accessor must return the SAME observer arc. The
    // pre-change tree (no substrate plumbing) returns `None` here ⇒
    // this assertion fails, discriminating against the gap.
    let observer = current_observer().expect(
        "post-change: substrate observer slot must be populated by RequestContextGuard::install",
    );

    // Emit an event through the substrate observer; the same arc is
    // both `Arc<dyn AuditObserver>` and `Arc<RequestContext>`, so
    // the per-request counter on `ctx` must increment.
    observer.record_event(AuditEvent::InflightAbortedRetry);
    assert_eq!(
        ctx.inflight_aborted_retries
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "AuditObserver::record_event must route through the same per-request counter \
         as the typed direct accessor; pre-change tree's empty-default impl on a separate \
         observer would leave the counter at 0"
    );

    observer.record_event(AuditEvent::ColdAbortSwept);
    assert_eq!(
        ctx.cold_aborts_swept
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[test]
fn substrate_current_observer_clears_after_guard_drops() {
    {
        let ctx = RequestContext::new(7, Arc::from("/x.vue"), false, None);
        let _guard = RequestContextGuard::install(ctx);
        // Mid-scope: observer present.
        assert!(
            current_observer().is_some(),
            "mid-scope: substrate observer must be present"
        );
    }
    // Post-drop: substrate slot must be empty (the guard's
    // `_audit_observer_guard` field drops via field destruction
    // order).
    assert!(
        current_observer().is_none(),
        "post-drop: substrate observer slot must be cleared by the guard's drop chain"
    );
}
