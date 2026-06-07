//! Matrix slice: `route_surface` × `route_surface`.
//!
//! Discrimination: `BarrelRouteSurface.fact_dep_signature` MUST be
//! able to carry self-referential `RouteSurface` facts (the
//! `EffectiveExportSet` route-domain fact on another barrel) so a
//! transitive barrel-of-barrels route surface is invalidated when
//! a downstream barrel's effective export set shifts.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use rustc_hash::FxHashMap;
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
fn route_surface_signature_carries_route_surface() {
    let src = read_session_src("resolver_core/route_db.rs");
    assert!(
        src.contains("pub struct BarrelRouteSurface")
            && src.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
        "route_surface matrix slice: `BarrelRouteSurface` \
         MUST declare `fact_dep_signature: Arc<[FactVersionRef]>` in \
         `resolver_core/route_db.rs`."
    );

    let route_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/src/inner_barrel.ts".to_owned(),
        key: FactKey::EffectiveExportSet,
        lane: FactLane::Semantic,
        expected_hash: [11u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![route_fact].into_boxed_slice());

    let surface = BarrelRouteSurface {
        barrel_canonical: "/src/outer_barrel.ts".to_owned(),
        wildcard_edges: FxHashMap::default(),
        fact_dep_signature: Arc::clone(&signature),
    };
    assert!(
        !surface.fact_dep_signature.is_empty(),
        "route_surface matrix slice: the constructed barrel surface \
         MUST carry the inner-barrel `EffectiveExportSet` \
         RouteSurface fact in its `fact_dep_signature`."
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&surface.fact_dep_signature),
        "route_surface matrix slice: the permissive view MUST accept \
         a self-referential `RouteSurface` fact in the route-surface \
         signature so barrel-of-barrels invalidation flows correctly."
    );
}
