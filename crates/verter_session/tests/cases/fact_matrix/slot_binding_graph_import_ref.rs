//! Matrix slice: `slot_binding_graph` × `import_ref`.
//!
//! Discrimination: the slot-binding-graph request fact tracer
//! fan-out substrate MUST deliver `FactKey::ImportRef` facts into
//! every active `FactReadSet`. Slot-payload types that resolve
//! through `import type { Slots } from './slots'` style references
//! produce these facts; the fan-out must carry them.

#![cfg(test)]

use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::{HostConfig, VerterHost};
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
use verter_session_query::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_session_query::facts::{FactKey, FactLane};

#[test]
fn slot_binding_graph_signature_carries_import_ref() {
    let host = VerterHost::new_standalone(HostConfig::default());

    let import_ref_fact = verter_session_query::facts::fact_cache::FactVersionRef::Parse(
        verter_session_query::facts::fact_cache::ParseFactRef {
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
        FactReadSetFinalise::NonCacheable(_) => panic!("fixture unexpectedly non-cacheable"),
        FactReadSetFinalise::Overflow | FactReadSetFinalise::MutationUnstable => panic!(
            "slot_binding_graph matrix slice: tracer overflowed \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &import_ref_fact),
        "slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `ImportRef` fact through the fan-out \
         path used by slot-binding-graph dependency tracing. \
         captured={captured:?}"
    );
}
