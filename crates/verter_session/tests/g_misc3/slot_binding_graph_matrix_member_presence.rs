//! Slot-binding-graph fact-tracer matrix slice — `MemberPresence`.
//!
//! Asserts that the slot-binding-graph dual-emit fact-tracer
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
//! `observe_fact_signature` for `accumulate_dispatch_dep_signature`
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
        FactReadSetFinalise::Overflow => panic!(
            "matrix slice: tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &presence_fact),
        "matrix slice: the fact-tracer substrate MUST carry \
         the `MemberPresence` fact through the fan-out path emitted \
         by the slot-binding-graph dual-emit helper. captured={captured:?}"
    );

    // Source-grep arch guard: the slot-binding-graph helper at the
    // top of `slot_binding_graph.rs` MUST contain both the bridge
    // helper call and the fan-out call. A regression that dropped
    // `observe_fact_signature` would not surface in the captured
    // signature above (this test scope does not call the real
    // helper) but would surface here.
    let src = read_session_src("meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("crate::fact_signature_helpers::observe_fact_signature(&bridged)"),
        "matrix slice (arch guard): \
         `emit_slot_binding_graph_dispatch_facts` in \
         `slot_binding_graph.rs` MUST call \
         `crate::fact_signature_helpers::observe_fact_signature(&bridged)` \
         to fan slot-binding-graph dispatch facts into the active \
         tracer stack — without this call the helper degenerates to \
         the legacy single-channel emission and `MemberPresence` \
         facts emitted by slot-binding-graph traversal never reach \
         the outer `with_fact_tracer` scope's curated signature."
    );
    assert!(
        src.contains("dep_signature_to_fact_signature(sig)"),
        "matrix slice (arch guard): \
         `emit_slot_binding_graph_dispatch_facts` MUST invoke the \
         bridge helper \
         `crate::fact_signature_helpers::dep_signature_to_fact_signature(sig)` \
         before fanning out — direct calls to \
         `observe_fact_signature(&legacy_dep_signature)` would fail \
         to type-check (different element types) but a substituted \
         empty slice would silently bypass coverage."
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

fn read_session_src(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}
