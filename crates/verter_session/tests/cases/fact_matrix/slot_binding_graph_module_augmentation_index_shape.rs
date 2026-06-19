//! Matrix slice: `slot_binding_graph` × `module_augmentation_index_shape`.
//!
//! Discrimination: the slot-binding-graph dual-emit fact-tracer
//! fan-out substrate MUST deliver
//! `FactVersionRef::RouteSurface(ModuleAugmentationIndexShape)` facts
//! into every active `FactReadSet`. Slot-payload types depending on
//! globally-augmented module surfaces (e.g.
//! `declare module 'vue' { ... }`) produce these facts; the fan-out
//! must carry them through the slot-binding-graph dispatch reads.

#![cfg(test)]

use verter_semantic::facts::registry::{AugmentationTargetKindTag, InternedSpecifier};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::for_tests::install_fact_tracer_for_tests;
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef, RouteSurfaceFactRef};
use verter_session::{HostConfig, VerterHost};

#[test]
fn slot_binding_graph_signature_carries_module_augmentation_index_shape() {
    let host = VerterHost::new_standalone(HostConfig::default());

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
            "slot_binding_graph matrix slice: tracer overflowed \
             on a single-fact signature — substrate bug, not test bug"
        ),
    };

    assert!(
        captured.iter().any(|f| f == &aug_index_fact),
        "slot_binding_graph matrix slice: the fact-tracer \
         substrate MUST carry the `ModuleAugmentationIndexShape` fact \
         through the fan-out path emitted by the slot-binding-graph \
         dual-emit helper. captured={captured:?}"
    );
}
