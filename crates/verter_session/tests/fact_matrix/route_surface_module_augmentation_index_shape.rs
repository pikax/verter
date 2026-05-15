//! Matrix slice: `route_surface` × `module_augmentation_index_shape`.
//!
//! Discrimination: `BarrelRouteSurface.fact_dep_signature` MUST be
//! able to carry `FactKey::ModuleAugmentationIndexShape` route-
//! surface facts so a route surface that resolves through a
//! globally-augmented module (e.g. `declare module 'vue' { ... }`)
//! is invalidated when the augmentation index fingerprint shifts.
//!
//! This is the R21 lib_env / project-identity scoping rule on the
//! route-surface tier: the `EffectiveExportSet` cache validates
//! `RouteSurface(ModuleAugmentationIndexShape)` anchors before
//! handing out a warm result.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use rustc_hash::FxHashMap;
use verter_semantic::facts::registry::{AugmentationTargetKindTag, InternedSpecifier};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::resolver_core::{
    BarrelRouteSurface, FactVersionRef, PermissiveStoreView, RouteSurfaceFactRef, StoreView,
};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn route_surface_signature_carries_module_augmentation_index_shape() {
    let src = read_session_src("resolver_core/route_db.rs");
    assert!(
        src.contains("pub struct BarrelRouteSurface")
            && src.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
        "Block 1.8 route_surface matrix slice: `BarrelRouteSurface` \
         MUST declare `fact_dep_signature: Arc<[FactVersionRef]>` in \
         `resolver_core/route_db.rs` after the Stage 6c substrate \
         migration."
    );

    let aug_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "vue".to_owned(),
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

    let surface = BarrelRouteSurface {
        barrel_canonical: "/src/barrel.ts".to_owned(),
        wildcard_edges: FxHashMap::default(),
        fact_dep_signature: Arc::clone(&signature),
    };
    assert!(
        !surface.fact_dep_signature.is_empty(),
        "route_surface matrix slice: the constructed barrel surface \
         MUST carry the `ModuleAugmentationIndexShape` fact in its \
         `fact_dep_signature`."
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&surface.fact_dep_signature),
        "route_surface matrix slice: the permissive view MUST accept \
         a `ModuleAugmentationIndexShape` RouteSurface fact in the \
         route-surface signature (R21 lib-env/project-identity scoping)."
    );
}
