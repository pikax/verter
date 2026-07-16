//! Matrix slice: `slot_binding_graph` × `route_surface`.
//!
//! Discrimination: the slot-binding-graph request fact tracer
//! fan-out substrate MUST deliver `RouteSurface` facts into every
//! active `FactReadSet`. The legacy `DepSignature` accumulator only
//! maps to `FileWholeHash`, so derived-domain `RouteSurface` facts
//! depend on the tracer path.

#![cfg(test)]

use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_signature_carries_route_surface() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let route_fact = verter_session::resolver_core::FactVersionRef::RouteSurface(
        verter_session::resolver_core::RouteSurfaceFactRef {
            canonical_id: "/src/slots.ts".to_owned(),
            key: FactKey::EffectiveExportSet,
            lane: FactLane::Semantic,
            expected_hash: [7u8; 16],
        },
    );

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &route_fact,
        ));
    });

    let captured = match finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow => panic!(
            "slot_binding_graph matrix slice: tracer overflowed \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &route_fact),
        "slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `EffectiveExportSet` RouteSurface \
         fact through the fan-out path emitted by the \
         slot-binding-graph dependency path. captured={captured:?}"
    );
}
