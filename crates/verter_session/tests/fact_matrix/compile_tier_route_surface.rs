//! Matrix slice: `compile_tier` × `route_surface`.
//!
//! Discrimination: `CompileSlot.fact_dep_signature` MUST be able to
//! carry `FactVersionRef::RouteSurface` facts so a consumer reading
//! through a barrel whose `EffectiveExportSet` shifts is invalidated
//! on the next warm read.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::{FactKey, FactLane};
use verter_session::resolver_core::{
    FactVersionRef, PermissiveStoreView, RouteSurfaceFactRef, StoreView,
};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn compile_tier_signature_carries_route_surface() {
    let src = read_session_src("types.rs");
    assert!(
        src.contains("fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1.8 compile_tier matrix slice: `CompileSlot` MUST \
         carry `fact_dep_signature: Arc<[FactVersionRef]>` after the \
         Block 1.A substrate migration."
    );

    let route_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/src/barrel.ts".to_owned(),
        key: FactKey::EffectiveExportSet,
        lane: FactLane::Semantic,
        expected_hash: [11u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![route_fact].into_boxed_slice());

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&signature),
        "compile_tier matrix slice: the permissive view MUST accept \
         an `EffectiveExportSet` RouteSurface fact in the Arc-stored \
         compile-slot signature so a barrel-shape shift invalidates \
         the consumer warm hit."
    );
}
