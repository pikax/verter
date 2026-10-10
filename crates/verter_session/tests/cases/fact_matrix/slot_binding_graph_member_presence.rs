//! Matrix slice: `slot_binding_graph` × `member_presence`.
//!
//! Discrimination: the slot-binding-graph request fact tracer
//! fan-out substrate MUST deliver `FactKey::MemberPresence` facts
//! into every active `FactReadSet` on the `ACTIVE_TRACERS` stack.
//! A regression dropping `observe_fact_signature` from the helper
//! would leave the captured signature empty.

#![cfg(test)]

use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::{HostConfig, VerterHost};
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
use verter_session_query::facts::registry::{InternedName, SymbolSpace};
use verter_session_query::facts::{FactKey, FactLane};

#[test]
fn slot_binding_graph_signature_carries_member_presence() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let presence_fact = verter_session_query::facts::fact_cache::FactVersionRef::Parse(
        verter_session_query::facts::fact_cache::ParseFactRef {
            canonical_id: "/src/slots.ts".to_owned(),
            key: FactKey::MemberPresence {
                exporter: InternedName::from("Slots"),
                name: verter_type_expr::facts::FactPropertyKey::identifier("default"),
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
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::MutationUnstable => panic!(
            "slot_binding_graph matrix slice: tracer was mutation-unstable \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &presence_fact),
        "slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `MemberPresence` fact through the \
         slot-binding dependency path. captured={captured:?}"
    );
}
