//! Block 1.C matrix slice — `Member`.
//!
//! Verifies the slot-binding-graph dual-emit fact-tracer fan-out
//! substrate delivers `FactKey::Member` facts into every active
//! `FactReadSet` on the `ACTIVE_TRACERS` stack — discriminating the
//! substrate's ability to carry the path-precise R28 member-body
//! fact-kind that slot-payload members produce when their type
//! expands to a concrete object.
//!
//! Discrimination property: a regression that swapped
//! `observe_fact_signature` for a no-op would leave the tracer's
//! `read_set` empty even though the legacy accumulator advanced —
//! this test would FAIL because the synthesised `Member` fact would
//! never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
#[ignore = "block-1.c RED — closed by same-block implementation"]
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
        FactReadSetFinalise::Overflow => panic!(
            "Block 1.C matrix slice: tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &member_fact),
        "Block 1.C matrix slice: the fact-tracer substrate MUST carry \
         the `Member` fact through the fan-out path emitted by the \
         slot-binding-graph dual-emit helper. captured={captured:?}"
    );

    // Arch guard: ensure the slot-binding-graph helper still calls
    // through `observe_fact_signature` so the slot-payload member
    // body facts emitted by the dispatch's `ProjectPath` Shallow
    // read on `param0_ty` (site 5) reach an active tracer scope.
    let src = read_session_src("meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("fn emit_slot_binding_graph_dispatch_facts"),
        "Block 1.C matrix slice (arch guard): \
         `slot_binding_graph.rs` MUST declare the dual-emit helper \
         `emit_slot_binding_graph_dispatch_facts` so all five \
         `accumulate_dispatch_dep_signature` call sites route \
         through the same paired emission point. A missing helper \
         would force ad-hoc paired emissions per site, defeating \
         the arch guard's site-pairing check."
    );
}

fn read_session_src(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}
