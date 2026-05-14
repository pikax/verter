//! Block 1.C matrix slice — `RouteSurface`.
//!
//! Verifies the slot-binding-graph dual-emit fact-tracer fan-out
//! substrate delivers `FactVersionRef::RouteSurface` facts —
//! cross-project import-route signatures — into every active
//! `FactReadSet`. Slot-payload types reached via cross-file imports
//! through tsconfig path aliases / package routes produce
//! `RouteSurface` facts; the fact-tracer fan-out must carry them
//! through the slot-binding-graph dispatch reads.
//!
//! Discrimination property: a regression that swapped
//! `observe_fact_signature` for a no-op would leave the tracer
//! empty even though the legacy accumulator advanced — this test
//! would FAIL because the synthesised `RouteSurface` fact would
//! never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_fact_tracer_carries_route_surface() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // `RouteSurface` is a derived-domain fact rather than a
    // parse-domain `ParseFactRef` — synthesize the derived variant
    // directly so the matrix slice exercises the same
    // `observe_fan_out_borrowed` substrate the slot-binding-graph
    // helper uses for parse-domain facts.
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
        FactReadSetFinalise::Overflow => panic!(
            "Block 1.C matrix slice: tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &route_fact),
        "Block 1.C matrix slice: the fact-tracer substrate MUST carry \
         the `RouteSurface` fact through the fan-out path emitted by \
         the slot-binding-graph dual-emit helper. captured={captured:?}"
    );

    // Arch guard: ensure `observe_fact_signature` (which calls
    // `observe_fan_out_borrowed`) is present in the helper. The
    // `RouteSurface` fact-kind is derived and would not survive the
    // legacy `accumulate_dispatch_dep_signature` path because
    // `DepVersion::WholeHash` only maps to `FileWholeHash` —
    // route-domain facts depend on the tracer path.
    let src = read_session_src("meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("observe_fact_signature(&bridged)"),
        "Block 1.C matrix slice (arch guard): \
         `slot_binding_graph.rs` MUST call \
         `observe_fact_signature(&bridged)` so RouteSurface and \
         other derived-domain facts reach the active tracer scope. \
         The legacy `DepSignature` accumulator drops everything but \
         whole-hash entries; the tracer fan-out is the sole channel \
         for derived-domain facts emitted within the \
         slot-binding-graph traversal."
    );
}

fn read_session_src(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}
