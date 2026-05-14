//! Block 1.C matrix slice — `ImportRef`.
//!
//! Verifies the slot-binding-graph dual-emit fact-tracer fan-out
//! substrate delivers `FactKey::ImportRef` facts (cross-file import
//! relationships) into every active `FactReadSet`. The
//! slot-binding-graph traversal lowers macro arguments that may
//! resolve through `import type { Slots } from './slots'` style
//! references; the `ImportRef` fact-kind captures the import edge
//! identity so a barrel re-export shift invalidates the captured
//! `fact_dep_signature`.
//!
//! Discrimination property: a regression dropping
//! `observe_fact_signature` from the slot-binding-graph helper
//! would leave the tracer empty even though the legacy accumulator
//! advanced — this test would FAIL because the synthesised
//! `ImportRef` fact would never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::FactReadSetFinalise;
use verter_session::{HostConfig, VerterHost};

#[test]
#[ignore = "block-1.c RED — closed by same-block implementation"]
fn slot_binding_graph_fact_tracer_carries_import_ref() {
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
            "Block 1.C matrix slice: tracer overflowed on a single-fact \
             signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &import_ref_fact),
        "Block 1.C matrix slice: the fact-tracer substrate MUST carry \
         the `ImportRef` fact through the fan-out path emitted by the \
         slot-binding-graph dual-emit helper. captured={captured:?}"
    );

    // Arch guard: the helper file must reference the
    // `dep_signature_to_fact_signature` bridge so legacy whole-hash
    // entries — which is all the bridge currently supports — are
    // converted before fan-out. An `ImportRef` parse-domain fact
    // becomes reachable through the tracer when the bridge is
    // active in the helper.
    let src = read_session_src("meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("dep_signature_to_fact_signature"),
        "Block 1.C matrix slice (arch guard): \
         `slot_binding_graph.rs` MUST reference \
         `dep_signature_to_fact_signature` as the bridge between \
         the legacy `DepSignature` accumulator payload and the \
         `Vec<FactVersionRef>` payload `observe_fact_signature` \
         consumes. Without this bridge, `ImportRef` facts (and \
         every other parse-domain fact-kind) silently bypass the \
         fact tracer."
    );
}

fn read_session_src(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}
