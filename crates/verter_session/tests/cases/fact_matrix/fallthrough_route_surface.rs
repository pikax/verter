//! Matrix slice: `fallthrough` × `route_surface`.
//!
//! Discrimination: `CachedFallthroughEntry.fact_versions` MUST be
//! able to carry `FactVersionRef::RouteSurface` facts so a fallthrough
//! consumer reading through a barrel is invalidated when the
//! barrel's `EffectiveExportSet` shifts.

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
fn fallthrough_signature_carries_route_surface() {
    let src = read_session_src("types.rs");
    let needle = "pub(crate) struct CachedFallthroughEntry {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = src[idx..]
        .find("\n}")
        .expect("CachedFallthroughEntry struct close");
    let window = &src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "fallthrough matrix slice: `CachedFallthroughEntry` \
         MUST carry `fact_versions: Arc<[FactVersionRef]>`. Window:\n{window}"
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
        "fallthrough matrix slice: the permissive view MUST accept an \
         `EffectiveExportSet` RouteSurface fact in the Arc-stored \
         fallthrough signature."
    );
}
