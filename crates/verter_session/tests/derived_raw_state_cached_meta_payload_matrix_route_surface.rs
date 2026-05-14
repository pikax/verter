//! Block 1A matrix slice — `DerivedRawState.cached_meta_payload`
//! must carry route-surface facts in its
//! `fact_versions: Arc<[FactVersionRef]>` signature so warm-hit
//! validation invalidates when an `EffectiveExportSet` flips (a
//! provider's exported names change).
//!
//! Pre-1A: wrapper holds `Vec<FactVersionRef>` and the source-grep
//! arch guard FAILS. Post-1A: substrate is `Arc<[FactVersionRef]>`
//! and a `RouteSurface` variant validates through the per-domain
//! fast-path.

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
fn cached_meta_payload_signature_carries_route_surface() {
    let src = read_session_src("types.rs");
    let needle = "pub(crate) struct CachedMetaPayload {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = src[idx..]
        .find("\n}")
        .expect("CachedMetaPayload struct close");
    let window = &src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1A matrix slice: CachedMetaPayload must carry \
         `fact_versions: Arc<[FactVersionRef]>` after the Block 1A migration. \
         Window:\n{window}"
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
        "permissive view must accept an EffectiveExportSet RouteSurface fact in the \
         Arc-stored signature"
    );
}
