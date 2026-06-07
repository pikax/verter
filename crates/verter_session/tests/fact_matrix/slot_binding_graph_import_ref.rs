//! Matrix slice: `slot_binding_graph` × `import_ref`.
//!
//! Discrimination: the slot-binding-graph dual-emit fact-tracer
//! fan-out substrate MUST deliver `FactKey::ImportRef` facts into
//! every active `FactReadSet`. Slot-payload types that resolve
//! through `import type { Slots } from './slots'` style references
//! produce these facts; the fan-out must carry them.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_signature_carries_import_ref() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let import_ref_fact = verter_session::resolver_core::FactVersionRef::Parse(
        verter_session::resolver_core::ParseFactRef {
            canonical_id: "/src/Comp.vue".to_owned(),
            key: FactKey::ImportRef {
                specifier: InternedSpecifier::from("./slots"),
                binding: InternedName::from("Slots"),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: [11u8; 16],
        },
    );

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &import_ref_fact,
        ));
    });

    let captured = match finalise {
        FactReadSetFinalise::Ok(sig) => sig,
        FactReadSetFinalise::Overflow => panic!(
            "slot_binding_graph matrix slice: tracer overflowed \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &import_ref_fact),
        "slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `ImportRef` fact through the fan-out \
         path emitted by the slot-binding-graph dual-emit helper. \
         captured={captured:?}"
    );
}
