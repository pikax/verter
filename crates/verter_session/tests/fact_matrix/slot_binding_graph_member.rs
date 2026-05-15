//! Matrix slice: `slot_binding_graph` × `member`.
//!
//! Discrimination: the slot-binding-graph dual-emit fact-tracer
//! fan-out substrate MUST deliver `FactKey::Member` facts into every
//! active `FactReadSet`.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_signature_carries_member() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let member_fact = verter_session::resolver_core::FactVersionRef::Parse(
        verter_session::resolver_core::ParseFactRef {
            canonical_id: "/src/slots.ts".to_owned(),
            key: FactKey::Member {
                exporter: InternedName::from("Slots"),
                name: InternedName::from("default"),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: [5u8; 16],
        },
    );

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &member_fact,
        ));
    });

    let captured = match finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::Overflow => panic!(
            "Block 1.8 slot_binding_graph matrix slice: tracer overflowed \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &member_fact),
        "Block 1.8 slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `Member` fact through the fan-out \
         path emitted by the slot-binding-graph dual-emit helper. \
         captured={captured:?}"
    );
}
