//! TLS-observer propagation through
//! [`verter_lsp::audit_harness::run_with_audit`].
//!
//! `run_with_audit` wraps every audited LSP handler future. When
//! audit is enabled it constructs an `LspAuditSession` (which
//! installs the `RequestContextGuard` for the duration of the
//! body), awaits the body under a timeout budget, and finalises the
//! session with the handler's payload. When audit is disabled it
//! short-circuits to `body.await` directly without constructing a
//! session.
//!
//! This test drives the wrapper through the
//! [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
//! harness and asserts:
//!
//! - **Positive** (`install_audit=true`, `audit_enabled=true` on the
//!   host): the synthetic body observes `current_observer() ==
//!   Some(_)` mid-flight; the wrapper publishes a record; and the
//!   harness's calling-thread observation is `Some` (the harness's
//!   outer `RequestContextGuard` remains visible after the
//!   wrapper's nested session drops on return).
//! - **Negative** (`install_audit=false`, `audit_enabled=false`):
//!   `run_with_audit` short-circuits to `body.await` directly; the
//!   body observes `current_observer() == None`; no record is
//!   published; the harness's calling-thread observation is `None`.
//!
//! Test structure: the harness is synchronous, but `run_with_audit`
//! is async. We use a synchronous `#[test]` that builds a
//! single-threaded Tokio runtime INSIDE the harness closure and
//! drives the wrapper's future via `Runtime::block_on`. This keeps
//! the harness's outer `RequestContextGuard` installed across the
//! `block_on` (the runtime's worker thread sees the calling thread's
//! TLS for synchronous `block_on` on a current-thread runtime) and
//! avoids the "cannot block within a runtime" trap that
//! `#[tokio::test]` would impose.
//!
//! Discrimination contract: the body's mid-flight `current_observer()`
//! probe returns `Some(_)` in the audit-enabled case and `None`
//! in the audit-disabled case.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use verter_audit::payloads::tags::LspMethodTag;
use verter_lsp::audit_harness;
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{HostConfig, VerterHost};

#[test]
fn run_with_audit_propagates_observer_into_handler_future() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        ..HostConfig::default()
    }));

    let observed = Arc::new(AtomicBool::new(false));
    let observed_clone = Arc::clone(&observed);
    let host_clone = Arc::clone(&host);

    let report = assert_observer_reaches(true, move || {
        // Build a single-threaded Tokio runtime inside the harness
        // closure. `block_on` on a `current_thread` runtime drives
        // the future on the calling thread, so the harness's outer
        // `RequestContextGuard` (installed on this thread) remains
        // visible inside the future. The wrapper's session install
        // nests on top.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("must build single-threaded Tokio runtime");
        let result = runtime.block_on(async {
            audit_harness::run_with_audit::<u8, _, _>(
                &host_clone,
                LspMethodTag::Hover,
                "/lsp_tls_probe.vue".to_string(),
                None,
                Duration::from_secs(5),
                async {
                    // Mid-flight probe: `run_with_audit` constructs
                    // the session BEFORE awaiting `body`, and the
                    // session installs the `RequestContextGuard`
                    // through `LspAuditSession::Active`. So inside
                    // this future, the substrate observer must be
                    // visible.
                    let saw = verter_audit::current_observer().is_some();
                    observed_clone.store(saw, Ordering::SeqCst);
                    Ok(7u8)
                },
                |payload, value| {
                    payload.response_size_bytes = u32::from(*value);
                },
            )
            .await
        });
        assert_eq!(
            result.expect("body must complete"),
            7,
            "body's measured value must flow through the wrapper unchanged",
        );
    });

    assert!(
        observed.load(Ordering::SeqCst),
        "synthetic handler future must see `current_observer().is_some()` — \
         a regression that drops the TLS plumbing in `LspAuditSession::Active::install` \
         (or in `RequestContextGuard::install`'s observer slot) would leave the \
         observer absent inside the body and this assertion would fail. \
         report = {report:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness's outer RequestContextGuard remains visible on the calling thread \
         after the entry-point's nested session drops on return: {report:?}",
    );

    // The wrapper must have published exactly one record (the LSP
    // session finalised with the populated payload).
    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap.records_store_size, 1,
        "run_with_audit must publish a record when audit is enabled — \
         a regression that swallows finalize_ok would leave the store empty"
    );
    assert_eq!(
        snap.active_request_count, 0,
        "post-state: the active-request registry must be drained by finalize",
    );
}

#[test]
fn run_with_audit_short_circuits_when_audit_disabled() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    assert!(!host.config().audit_enabled);

    let observed = Arc::new(AtomicBool::new(true));
    let observed_clone = Arc::clone(&observed);
    let host_clone = Arc::clone(&host);

    let report = assert_observer_reaches(false, move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("must build single-threaded Tokio runtime");
        let result = runtime.block_on(async {
            audit_harness::run_with_audit::<u8, _, _>(
                &host_clone,
                LspMethodTag::Hover,
                "/lsp_tls_probe.vue".to_string(),
                None,
                Duration::from_secs(5),
                async {
                    // `run_with_audit` checks `host.config().audit_enabled`
                    // and short-circuits to `body.await` directly when
                    // audit is disabled, without constructing a
                    // session — so no `RequestContextGuard` is
                    // installed and the substrate observer must be
                    // absent inside the body.
                    let saw = verter_audit::current_observer().is_some();
                    observed_clone.store(saw, Ordering::SeqCst);
                    Ok(1u8)
                },
                |payload, _value| {
                    payload.response_size_bytes = 1;
                },
            )
            .await
        });
        assert_eq!(
            result.expect("audit-disabled body should pass through"),
            1,
            "audit-disabled body must still return the value unchanged",
        );
    });

    assert!(
        !observed.load(Ordering::SeqCst),
        "with audit disabled, `run_with_audit` short-circuits to `body.await` \
         and the body must observe `current_observer() == None` — \
         a regression that left the session-install path on the audit-disabled \
         branch would surface here as a stray `Some` observer. \
         report = {report:?}",
    );
    assert!(
        !report.observer_seen_on_calling_thread,
        "harness installed no outer guard and the entry-point short-circuited \
         to the body without installing one either; the calling thread must \
         see no observer: {report:?}",
    );

    let snap = host.host_audit_runtime().snapshot();
    assert_eq!(
        snap.records_store_size, 0,
        "audit_enabled=false ⇒ no record must enter the records store",
    );
    assert_eq!(snap.active_request_count, 0);
}
