//! Matrix slice: `compile_tier` × `module_augmentation_index_shape`.
//!
//! Discrimination: `CompileSlot.fact_dep_signature` MUST be the
//! `ReadSetSignature` carrier and able to carry
//! `FactKey::ModuleAugmentationIndexShape` route-surface facts
//! so a consumer that depends on a globally-augmented module surface
//! (e.g. `declare module 'vue' { ... }`) is invalidated when the
//! augmentation index fingerprint shifts.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_session::resolver_core::PermissiveStoreView;
use verter_session_query::facts::fact_cache::{FactVersionRef, RouteSurfaceFactRef};
use verter_session_query::facts::registry::{AugmentationTargetKindTag, InternedSpecifier};
use verter_session_query::facts::store_view::StoreView;
use verter_session_query::facts::{FactKey, FactLane};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn compile_tier_signature_carries_module_augmentation_index_shape() {
    let src = read_session_src("types.rs");
    assert!(
        src.contains(
            "fact_dep_signature: verter_session_query::facts::fact_cache::ReadSetSignature"
        ),
        "compile_tier matrix slice: `CompileSlot` MUST carry \
         `fact_dep_signature: ReadSetSignature` (the carrier that wraps \
         `Arc<[FactVersionRef]>` + the overflow flag). A regression that \
         drops the typed carrier would bypass per-domain fact validation."
    );

    let aug_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/src/Comp.vue".to_owned(),
        key: FactKey::ModuleAugmentationIndexShape {
            target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
            external_specifier: Some(InternedSpecifier::from("vue")),
            resolved_relative_canonical: None,
            wildcard_pattern: None,
        },
        lane: FactLane::Semantic,
        expected_hash: [13u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![aug_fact].into_boxed_slice());

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&signature),
        "compile_tier matrix slice: the permissive view MUST accept \
         a `ModuleAugmentationIndexShape` RouteSurface fact in the \
         Arc-stored compile-slot signature so an augmentation-index \
         shift invalidates the consumer warm hit."
    );
}
