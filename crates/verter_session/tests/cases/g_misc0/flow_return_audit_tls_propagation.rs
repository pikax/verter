//! TLS-observer propagation through
//! `VerterHost::get_flow_return_type_with_audit`.
//!
//! Drives the public production entry-point through the
//! [`verter_session::tests::audit_tls_harness::assert_observer_reaches`]
//! harness and asserts the audit substrate's TLS slot is populated for
//! the duration of the audited window:
//!
//! - **Positive**: `assert_observer_reaches(true, …)` — the harness
//!   installs an outer `RequestContextGuard`, the entry-point installs
//!   its own (Active) registration's guard inside, the cold flow
//!   evaluation reaches the `FlowReturnStarted` emission site with an
//!   installed request context, and the published record's
//!   `cold_computes` counter is non-zero — proving the context was
//!   reachable on the dispatch path.
//! - **Negative**: `assert_observer_reaches(false, …)` — the harness
//!   installs no outer guard; after the entry-point's nested guard
//!   drops on return, the calling thread must see
//!   `current_observer() == None`.
//!
//! Discrimination contract: a regression that left the TLS slot
//! unpopulated during the audited window would leave
//! `cold_computes == 0` on a COLD inference (the counter bumps only
//! through `current_request_context()`), failing the positive case; a
//! regression leaking the entry-point's guard past its return fails
//! the negative case.

use std::sync::Arc;

use verter_session::semantic_query::ReturnProjectionDemand;
use verter_session::tests::audit_tls_harness::assert_observer_reaches;
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const CANONICAL: &str = "/w/flow-tls.ts";

const FIXTURE: &str = r#"
export function makeThing() {
  return { ok: "yes" };
}
"#;

fn build_host(audit_enabled: bool) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled,
        footprint_capture: false,
        ..HostConfig::default()
    }));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(FIXTURE),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn make_thing_identity() -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("makeThing"),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

#[test]
fn get_flow_return_type_with_audit_propagates_observer_through_dispatch() {
    let host = build_host(true);

    let mut record_kind: Option<verter_audit::RequestKind> = None;
    let mut cold_computes: u32 = 0;
    let report = assert_observer_reaches(true, || {
        // Drive the real production audited entry-point. The cold
        // flow evaluation bumps `flow_return_cold_computes` through
        // `current_request_context()` at the `FlowReturnStarted`
        // emission site — non-zero ⇒ the request context installed by
        // the entry-point's guard was reachable on the dispatch path.
        let (resolved, record) = host
            .get_flow_return_type_with_audit(
                &make_thing_identity(),
                ReturnProjectionDemand::whole_return(),
            )
            .into_parts();
        assert!(
            resolved.is_ok(),
            "cold flow inference of `makeThing` must succeed under audit"
        );
        record_kind = Some(record.kind.clone());
        if let Some(payload) = record.flow_return_inference_payload() {
            cold_computes = payload.cold_computes;
        }
    });

    assert!(
        matches!(
            record_kind,
            Some(verter_audit::RequestKind::FlowReturnInference)
        ),
        "get_flow_return_type_with_audit must publish a FlowReturnInference record when \
         audit is enabled. record_kind = {record_kind:?}, report = {report:?}",
    );
    assert!(
        cold_computes > 0,
        "the cold evaluation's counter must increment under audit; got \
         cold_computes={cold_computes}. A regression that left the request context \
         uninstalled on the dispatch path would leave the per-request counter at 0. \
         report = {report:?}",
    );
    assert!(
        report.observer_seen_on_calling_thread,
        "harness must record the calling-thread observation as Some — the harness's outer \
         RequestContextGuard remains installed when the entry-point's nested guard drops on \
         return: {report:?}",
    );
}

#[test]
fn get_flow_return_type_with_audit_observer_absent_outside_harness_window() {
    // Like `resolve_type_with_audit`, this entry-point is
    // filter-driven: the consumer-filter snapshot defaults to
    // allow-all, so the registration takes the `Active` arm even with
    // `audit_enabled=false`. The harness-drivable discriminator is
    // the OUTSIDE-the-window observation: with `install_audit=false`
    // the harness installs no outer guard, so after the entry-point's
    // own guard drops on return, the calling thread must see no
    // observer.
    let host = build_host(false);

    let report = assert_observer_reaches(false, || {
        let (_resolved, _record) = host
            .get_flow_return_type_with_audit(
                &make_thing_identity(),
                ReturnProjectionDemand::whole_return(),
            )
            .into_parts();
    });

    assert!(
        !report.observer_seen_on_calling_thread,
        "harness installed no outer guard; the entry-point's nested guard MUST drop on \
         return, so the calling thread must see no observer at the harness's post-call \
         probe point. report = {report:?}",
    );
}
