//! `FactKey::Member` fan-out through the slot-binding-graph tracer.
//!
//! Verifies the slot-binding-graph fact-tracer fan-out
//! substrate delivers `FactKey::Member` facts into every active
//! `FactReadSet` on the `ACTIVE_TRACERS` stack — discriminating the
//! substrate's ability to carry the path-precise R28 member-body
//! fact-kind that slot-payload members produce when their type
//! expands to a concrete object.
//!
//! Discrimination property: a regression that swapped
//! `observe_fact_signature` for a no-op would leave the tracer's
//! `read_set` empty; this test would fail because the synthesised `Member` fact would
//! never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_fact_tracer_carries_member() {
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
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow => panic!(
            "tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &member_fact),
        "the fact-tracer substrate MUST carry \
         the `Member` fact through the fan-out path emitted by the \
         slot-binding-graph dependency path. captured={captured:?}"
    );
}
