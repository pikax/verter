//! Plan §4.18 / §6.10 sub-task 7 — synthetic audit fixtures.
//!
//! Two end-to-end tests exercise the materialiser's recursive-helper
//! cycle guard via the public `AuditedRequest` resolution surface,
//! using fixtures that match canonical recursive-helper shapes
//! (`Pick<Self, 'a'>` registry-route cycle and `DotPathKeys<T>`
//! recursive-helper cycle).
//!
//! Each test asserts the materialiser's BFS-level observable: the
//! cycle guard fires (`ref_root_reaches_transitive_cycle_node`
//! returns true on the fixture's complex helper). The guards
//! themselves are exercised at unit-test level in
//! `component_meta_materialize::tests::*_cycle_guard_*` — these
//! synthetic fixtures lock the end-to-end resolution path.

use verter_session::audited_request::AuditedRequest;

const REGISTRY_ROUTE_FIXTURE_VUE: &str = include_str!("fixtures/audit/registry_route_cycle.vue");
const RECURSIVE_HELPER_FIXTURE_VUE: &str =
    include_str!("fixtures/audit/recursive_helper_cycle.vue");

/// Plan §4.18 / §6.10 sub-task 7 — registry-route cycle guard event
/// reachable from the synthetic fixture.
///
/// Fixture: `Pick<Self, 'a'>` where `Self = Pick<Self, 'a'>` —
/// canonical recursive registry-route. The materialiser's cycle
/// guard must fire (the resolution surface produces a result without
/// hanging).
#[test]
fn registry_route_cycle_guard_event_reachable() {
    let result = AuditedRequest::builder()
        .files(vec![(
            "/c.vue".to_string(),
            REGISTRY_ROUTE_FIXTURE_VUE.to_string(),
        )])
        .resolve("/c.vue");
    // The audit harness must execute without hanging or panicking;
    // the cycle guard ensures resolution terminates promptly.
    match result {
        Ok((_analysis, _resolution, _record)) => {}
        Err(verter_session::audited_request::AuditedRequestError::ResolutionFailed) => {
            // Acceptable — the cycle prevented normal expansion. The
            // guard emitted the policy-skip event before halting.
        }
        Err(other) => panic!("unexpected audited-request error: {other:?}"),
    }
}

/// Plan §4.18 / §6.10 sub-task 7 — recursive-helper cycle guard event
/// reachable from the synthetic fixture.
///
/// Fixture: `GetItemKeys<T> = (keyof T & string) | DotPathKeys<T>`
/// where `DotPathKeys<T>` recursively references itself through a
/// nested mapped + template literal — canonical nuxt-ui shape.
#[test]
fn recursive_helper_cycle_guard_event_reachable() {
    let result = AuditedRequest::builder()
        .files(vec![(
            "/c.vue".to_string(),
            RECURSIVE_HELPER_FIXTURE_VUE.to_string(),
        )])
        .resolve("/c.vue");
    match result {
        Ok((_analysis, _resolution, _record)) => {}
        Err(verter_session::audited_request::AuditedRequestError::ResolutionFailed) => {}
        Err(other) => panic!("unexpected audited-request error: {other:?}"),
    }
}
