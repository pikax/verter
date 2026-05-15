//! Matrix slice: `route_surface` × `import_ref`.
//!
//! Discrimination: `BarrelRouteSurface.fact_dep_signature` MUST be
//! able to carry `FactKey::ImportRef` facts so a barrel-route
//! surface entry is invalidated when the importer's
//! binding-to-export resolution shifts.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use rustc_hash::FxHashMap;
use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::resolver_core::{
    BarrelRouteSurface, FactVersionRef, ParseFactRef, PermissiveStoreView, StoreView,
};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn route_surface_signature_carries_import_ref() {
    let src = read_session_src("resolver_core/route_db.rs");
    assert!(
        src.contains("pub struct BarrelRouteSurface")
            && src.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
        "Block 1.8 route_surface matrix slice: `BarrelRouteSurface` \
         MUST declare `fact_dep_signature: Arc<[FactVersionRef]>` in \
         `resolver_core/route_db.rs` after the Stage 6c substrate \
         migration."
    );

    let import_fact = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/src/barrel.ts".to_owned(),
        key: FactKey::ImportRef {
            specifier: InternedSpecifier::from("./leaf"),
            binding: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [3u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![import_fact].into_boxed_slice());

    let surface = BarrelRouteSurface {
        barrel_canonical: "/src/barrel.ts".to_owned(),
        wildcard_edges: FxHashMap::default(),
        fact_dep_signature: Arc::clone(&signature),
    };
    assert!(
        !surface.fact_dep_signature.is_empty(),
        "route_surface matrix slice: the constructed barrel surface \
         MUST carry the `ImportRef` fact in its `fact_dep_signature`."
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&surface.fact_dep_signature),
        "route_surface matrix slice: the permissive view MUST accept \
         an `ImportRef` fact in the route-surface signature."
    );
}
