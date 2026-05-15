//! Matrix slice: `slot_binding_graph` × `member_presence`.
//!
//! Discrimination: the slot-binding-graph dual-emit fact-tracer
//! fan-out substrate MUST deliver `FactKey::MemberPresence` facts
//! into every active `FactReadSet` on the `ACTIVE_TRACERS` stack.
//! A regression dropping `observe_fact_signature` from the helper
//! would leave the captured signature empty.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_signature_carries_member_presence() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let presence_fact = verter_session::resolver_core::FactVersionRef::Parse(
        verter_session::resolver_core::ParseFactRef {
            canonical_id: "/src/slots.ts".to_owned(),
            key: FactKey::MemberPresence {
                exporter: InternedName::from("Slots"),
                name: InternedName::from("default"),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: [3u8; 16],
        },
    );

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &presence_fact,
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
        captured.iter().any(|f| f == &presence_fact),
        "Block 1.8 slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `MemberPresence` fact through the \
         fan-out path emitted by the slot-binding-graph dual-emit \
         helper. captured={captured:?}"
    );
}
