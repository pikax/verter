//! Behavioral contract for the audited flow-return host entry-point
//! (`VerterHost::get_flow_return_type_with_audit`) — the
//! `U6.FLOW_RETURN_SUBSTRATE` cold-vs-warm audit exit acceptance:
//!
//! - a COLD inference emits `FlowReturnStarted` (exactly one for a
//!   self-contained function), publishes a `FlowReturnInference`
//!   record with `cold_computes >= 1`, and reports
//!   `from_cache = false`;
//! - a WARM family hit emits NO `FlowReturnStarted`, reports
//!   `from_cache = true`, and `cold_computes == 0`;
//! - a flow-slice budget refusal surfaces the typed
//!   `FlowReturnFailure::Budget` on the carrier's `Err` arm, bumps
//!   `budget_exceeded_events`, and emits `FlowSliceBudgetExceeded`;
//! - direct recursion records the flow-cycle sentinel
//!   (`cycle_reentry_holds >= 1` + `FlowCycleSentinelHit`);
//! - a narrower-than-whole-return demand fails CLOSED with the typed
//!   `UnmodeledDemandPoint` failure (never a silently widened
//!   whole-return result);
//! - with audit disabled the producer body still runs and the carrier
//!   returns the cheap default-filled record
//!   (`AuditCaptureState::AuditDisabled`, nothing published).
//!
//! Discrimination: against a tree that emits `FlowReturnStarted` on
//! the warm path, `warm_hit_emits_no_flow_return_started_event` fails
//! on its exactly-zero assertion; against a tree that never bumps the
//! cold counter, the cold-path test fails on `cold_computes >= 1`.

use std::sync::Arc;

use verter_audit::{AuditCaptureState, RequestKind, StructuredAuditEvent};
use verter_session::host_flow_return_audit::FlowReturnError;
use verter_session::semantic_query::{demand, FlowReturnFailure, ReturnProjectionDemand};
use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const CANONICAL: &str = "/w/flow-audit.ts";

const FIXTURE: &str = r#"
export function makeThing() {
  return { ok: "yes" };
}

export function recurse(n: number) {
  if (n <= 0) return 0;
  return recurse(n - 1);
}
"#;

fn build_host(audit_enabled: bool, footprint_capture: bool) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled,
        footprint_capture,
        ..HostConfig::default()
    }));
    upsert(&host, CANONICAL, FIXTURE);
    host
}

fn upsert(host: &Arc<VerterHost>, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn identity(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

/// Count `FlowReturnStarted` events on a record's mined footprint.
fn flow_return_started_count(record: &verter_audit::RequestAuditRecord) -> usize {
    record
        .footprint
        .as_ref()
        .map(|fp| {
            fp.structured_events
                .iter()
                .filter(|ev| matches!(ev, StructuredAuditEvent::FlowReturnStarted { .. }))
                .count()
        })
        .unwrap_or(0)
}

fn has_event(
    record: &verter_audit::RequestAuditRecord,
    pred: impl Fn(&StructuredAuditEvent) -> bool,
) -> bool {
    record
        .footprint
        .as_ref()
        .is_some_and(|fp| fp.structured_events.iter().any(&pred))
}

#[test]
fn cold_inference_emits_started_event_and_counts_one_cold_compute() {
    let host = build_host(true, true);
    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), whole());

    let record = carrier.audit();
    assert!(
        carrier.as_result().is_ok(),
        "self-contained object return must resolve complete"
    );
    assert_eq!(record.kind, RequestKind::FlowReturnInference);
    assert_eq!(record.capture_state, AuditCaptureState::ActiveStored);
    assert!(!record.from_cache, "a cold inference is not a cache hit");

    let payload = record
        .flow_return_inference_payload()
        .expect("FlowReturnInference record carries the flow payload");
    assert_eq!(payload.function_symbol, "makeThing");
    assert!(
        payload.cold_computes >= 1,
        "the cold evaluation must bump cold_computes; got {}",
        payload.cold_computes
    );
    assert_eq!(payload.budget_exceeded_events, 0);
    assert_eq!(payload.cycle_reentry_holds, 0);

    assert_eq!(
        flow_return_started_count(record),
        1,
        "a self-contained function cold-evaluates exactly once and emits exactly \
         one FlowReturnStarted"
    );

    // The active registration published the record into the host store.
    assert!(
        host.take_audit_record(record.request_id).is_some(),
        "active flow-return registration must publish into the records store"
    );
}

#[test]
fn warm_hit_emits_no_flow_return_started_event() {
    let host = build_host(true, true);
    let ident = identity(CANONICAL, "makeThing");

    // Prime: cold inference publishes the family memo entry.
    let cold = host.get_flow_return_type_with_audit(&ident, whole());
    assert!(cold.as_result().is_ok(), "cold prime must resolve");
    assert!(!cold.audit().from_cache);

    // Warm: the family hit returns before any frame opens.
    let warm = host.get_flow_return_type_with_audit(&ident, whole());
    let record = warm.audit();
    assert!(warm.as_result().is_ok(), "warm hit must resolve");
    assert!(
        record.from_cache,
        "a zero-cold-compute success is a warm hit and must report from_cache"
    );
    let payload = record
        .flow_return_inference_payload()
        .expect("flow payload");
    assert_eq!(
        payload.cold_computes, 0,
        "a warm family hit runs zero cold evaluations"
    );
    // THE exit-acceptance negative: no FlowReturnStarted on the warm path.
    assert_eq!(
        flow_return_started_count(record),
        0,
        "a warm family hit must emit NO FlowReturnStarted event"
    );
}

#[test]
fn budget_refusal_surfaces_typed_error_counter_and_event() {
    let host = build_host(true, true);
    // 300 demand-origin return sites trips the ReturnSites budget (256).
    let canonical = "/w/flow-audit-budget.ts";
    let mut source = String::from("export function tooManyReturns(n: number) {\n");
    for i in 0..300 {
        source.push_str(&format!("  if (n === {i}) return {i};\n"));
    }
    source.push_str("  return -1;\n}\n");
    upsert(&host, canonical, &source);

    let carrier =
        host.get_flow_return_type_with_audit(&identity(canonical, "tooManyReturns"), whole());
    let record = carrier.audit();
    match carrier.as_result() {
        Err(FlowReturnError::Failure(FlowReturnFailure::Budget(_))) => {}
        other => panic!("expected typed Budget failure on the Err arm, got {other:?}"),
    }
    assert!(!record.from_cache, "a refused result is never a cache hit");
    let payload = record
        .flow_return_inference_payload()
        .expect("flow payload");
    assert!(
        payload.budget_exceeded_events >= 1,
        "the budget refusal must bump budget_exceeded_events"
    );
    assert!(
        has_event(record, |ev| matches!(
            ev,
            StructuredAuditEvent::FlowSliceBudgetExceeded { .. }
        )),
        "the budget refusal must emit FlowSliceBudgetExceeded"
    );
}

#[test]
fn direct_recursion_records_cycle_sentinel() {
    let host = build_host(true, true);
    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "recurse"), whole());
    let record = carrier.audit();
    assert!(
        carrier.as_result().is_ok(),
        "base-plus-recursion admits a widened result"
    );
    let payload = record
        .flow_return_inference_payload()
        .expect("flow payload");
    assert!(
        payload.cycle_reentry_holds >= 1,
        "the direct self-call must record a coinductive re-entry hold"
    );
    assert!(
        has_event(record, |ev| matches!(
            ev,
            StructuredAuditEvent::FlowCycleSentinelHit { .. }
        )),
        "the re-entry must emit FlowCycleSentinelHit"
    );
}

#[test]
fn narrower_demand_fails_closed_with_unmodeled_demand_point() {
    let host = build_host(true, false);
    let narrower = ReturnProjectionDemand {
        point: demand::Demand::navigate(demand::ProjectionPath::empty()),
    };
    assert!(
        !narrower.is_whole_return(),
        "test precondition: the navigate point differs from whole-return"
    );
    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), narrower);
    match carrier.as_result() {
        Err(FlowReturnError::Failure(FlowReturnFailure::UnmodeledDemandPoint)) => {}
        other => panic!(
            "a narrower-than-whole-return demand must fail CLOSED with \
             UnmodeledDemandPoint, got {other:?}"
        ),
    }
}

#[test]
fn filtered_kind_returns_cheap_noop_record_and_publishes_nothing() {
    // Like the sibling entry-points, the Active/Noop split is
    // FILTER-driven: deny the kind at the consumer filter and the
    // registration takes the `Noop` arm — the producer body still
    // runs, the returned record is the cheap default-filled envelope,
    // and nothing enters the records store.
    let mut host = VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: false,
        ..HostConfig::default()
    });
    host.replace_host_audit_runtime_for_test(verter_audit::AuditConfig {
        consumer_filter: verter_audit::AuditConsumerFilter::deny_all(),
        ..verter_audit::AuditConfig::default()
    });
    let host = Arc::new(host);
    upsert(&host, CANONICAL, FIXTURE);

    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), whole());
    let record = carrier.audit();
    assert!(
        carrier.as_result().is_ok(),
        "the producer body runs regardless of audit state"
    );
    assert_eq!(record.kind, RequestKind::FlowReturnInference);
    assert_eq!(record.capture_state, AuditCaptureState::FilteredNoop);
    assert!(record.footprint.is_none(), "no footprint on the noop arm");
    let payload = record
        .flow_return_inference_payload()
        .expect("noop record still carries the default flow payload");
    assert_eq!(payload.cold_computes, 0, "noop record is default-filled");
    // Nothing was published into the records store.
    assert!(
        host.take_audit_record(record.request_id).is_none(),
        "a filtered registration must not publish"
    );
}

fn whole() -> ReturnProjectionDemand {
    ReturnProjectionDemand::whole_return()
}
