//! Block 1.C matrix slice — `ModuleAugmentationIndexShape`.
//!
//! Verifies the slot-binding-graph dual-emit fact-tracer fan-out
//! substrate delivers `FactVersionRef::ModuleAugmentationIndexShape`
//! facts — the per-project augmentation-index shape signature —
//! into every active `FactReadSet`. Slot-payload types that depend
//! on globally-augmented module surfaces (e.g.
//! `declare module 'vue' { ... }`) produce these facts; the
//! fact-tracer fan-out must carry them through the
//! slot-binding-graph dispatch reads.
//!
//! Discrimination property: a regression that dropped the bridge
//! `dep_signature_to_fact_signature` would leave the tracer empty
//! even though the legacy accumulator advanced — this test would
//! FAIL because the synthesised `ModuleAugmentationIndexShape`
//! fact would never reach the tracer.

#![cfg(test)]

use verter_semantic::facts::registry::{AugmentationTargetKindTag, InternedSpecifier};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef, RouteSurfaceFactRef};
use verter_session::{HostConfig, VerterHost};

#[test]
#[ignore = "block-1.c RED — closed by same-block implementation"]
fn slot_binding_graph_fact_tracer_carries_module_aug_index_shape() {
    let host = VerterHost::new_standalone(HostConfig::default());

    // `ModuleAugmentationIndexShape` is encoded as a
    // `RouteSurfaceFactRef` whose `key` carries the
    // `FactKey::ModuleAugmentationIndexShape` variant (route-domain
    // facts validate against the augmentation index). The fact-
    // tracer fan-out must round-trip this variant unchanged so
    // augmentation-shape shifts reach the captured signature when
    // a slot-payload type depends on a globally-augmented module.
    let aug_index_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/src/Comp.vue".to_owned(),
        key: FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
            external_specifier: Some(InternedSpecifier::from("vue")),
            resolved_relative_canonical: None,
            wildcard_pattern: None,
        },
        lane: FactLane::Semantic,
        expected_hash: [17u8; 16],
    });

    let ((), finalise) = install_fact_tracer_for_tests(&host, || {
        verter_session::for_tests::observe_fan_out_borrowed_for_tests(std::slice::from_ref(
            &aug_index_fact,
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
        captured.iter().any(|f| f == &aug_index_fact),
        "Block 1.C matrix slice: the fact-tracer substrate MUST carry \
         the `ModuleAugmentationIndexShape` fact through the fan-out \
         path emitted by the slot-binding-graph dual-emit helper. \
         captured={captured:?}"
    );

    // Arch guard: the helper must route through the same
    // `observe_fan_out_borrowed` substrate the tracer captures, so
    // augmentation-index shape shifts reach the captured signature.
    let src = read_session_src("meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("fact_signature_helpers::observe_fact_signature"),
        "Block 1.C matrix slice (arch guard): \
         `slot_binding_graph.rs` MUST route through \
         `fact_signature_helpers::observe_fact_signature` so the \
         `ModuleAugmentationIndexShape` fact-kind (and every other \
         derived-domain fact-kind the tracer carries) reaches the \
         active tracer stack from slot-binding-graph dispatch reads."
    );
}

fn read_session_src(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}
