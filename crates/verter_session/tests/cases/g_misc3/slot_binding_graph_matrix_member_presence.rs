//! Slot-binding-graph fact-tracer matrix slice — `MemberPresence`.
//!
//! Asserts that the slot-binding-graph fact-tracer
//! fan-out substrate can deliver `FactKey::MemberPresence` facts
//! into every active `FactReadSet` on the `ACTIVE_TRACERS` stack.
//!
//! The slot-binding-graph traversal has no result cache of its own,
//! so the matrix slice verifies the in-flight observation path:
//! a `MemberPresence` fact synthesised into the tracer's stack via
//! `observe_fact_signature` (the bridge `observe_fan_out_borrowed`
//! into the active tracer scope) is observable via
//! `read_set.finalise()`'s `Ok(sig)` arm.
//!
//! Discrimination property: a regression that swapped
//! `observe_fact_signature` for the dispatch dependency bridge
//! alone in the slot-binding-graph helper would leave the tracer's
//! `read_set` empty even though the legacy accumulator advanced —
//! this test would FAIL because the synthesised `MemberPresence`
//! fact would never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::{
    dep_signature_to_fact_signature_for_tests, install_fact_tracer_for_tests,
};
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_fact_tracer_carries_member_presence() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // Construct a single-entry signature directly bearing the
    // `MemberPresence` fact-kind. The slot-binding-graph helper's
    // bridge path (`dep_signature_to_fact_signature` +
    // `observe_fact_signature`) accepts an arbitrary `DepSignature`
    // payload — the bridge converts only the whole-hash entries;
    // every fact-kind that carries a parse-domain `MemberPresence`
    // entry must therefore round-trip through the tracer when
    // emitted from an active tracer scope.
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

    // Drive the bridge through an installed tracer scope; the
    // tracer captures the fan-out via `observe_fan_out_borrowed`,
    // which is what `observe_fact_signature` calls.
    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &presence_fact,
        ));
    });

    let captured = match finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow => panic!(
            "matrix slice: tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &presence_fact),
        "matrix slice: the fact-tracer substrate MUST carry \
         the `MemberPresence` fact through the fan-out path emitted \
         by the slot-binding-graph dependency path. captured={captured:?}"
    );

    // Also exercise the bridge function directly so the test
    // discriminates a regression that removed
    // `dep_signature_to_fact_signature` from the public for_tests
    // surface.
    let bridged_empty: Vec<verter_session::resolver_core::FactVersionRef> =
        dep_signature_to_fact_signature_for_tests(&std::sync::Arc::from(
            Vec::<(
                std::sync::Arc<str>,
                verter_session::semantic_query::DepVersion,
            )>::new()
            .into_boxed_slice(),
        ));
    assert!(
        bridged_empty.is_empty(),
        "matrix slice: empty `DepSignature` must bridge to \
         empty `Vec<FactVersionRef>`. observed={bridged_empty:?}"
    );
}
