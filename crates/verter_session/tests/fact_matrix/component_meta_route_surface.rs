//! Matrix slice: `component_meta` × `route_surface`.
//!
//! Discrimination: `ComponentMetaResultEntry.read_set_signature.facts`
//! MUST be able to carry `RouteSurface` facts (per-domain Route facts)
//! so the final-result cache invalidates when a barrel's effective
//! export set changes (e.g. a wildcard reexport's target retargets).

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::{FactKey, FactLane};
use verter_session::component_meta_result_db::ComponentMetaResultEntry;
use verter_session::for_tests::ReadSetSignature;
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
fn component_meta_signature_carries_route_surface() {
    let src = read_session_src("component_meta_result_db.rs");
    let needle = "pub struct ComponentMetaResultEntry<P> {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in component_meta_result_db.rs"));
    let end = src[idx..]
        .find("\n}")
        .expect("ComponentMetaResultEntry struct close");
    let window = &src[idx..idx + end];
    assert!(
        window.contains("read_set_signature: crate::fact_signature_helpers::ReadSetSignature")
            || window.contains("read_set_signature: ReadSetSignature"),
        "component_meta matrix slice: \
         `ComponentMetaResultEntry` MUST carry \
         `read_set_signature: ReadSetSignature`. Window:\n{window}"
    );

    let route_fact = FactVersionRef::RouteSurface(RouteSurfaceFactRef {
        canonical_id: "/src/barrel.ts".to_owned(),
        key: FactKey::EffectiveExportSet,
        lane: FactLane::Semantic,
        expected_hash: [11u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![route_fact].into_boxed_slice());

    let entry: ComponentMetaResultEntry<u32> = ComponentMetaResultEntry {
        payload: Arc::new(0u32),
        read_set_signature: ReadSetSignature::new(Arc::clone(&signature)),
        validated_at_generation: 0,
    };
    assert!(
        !entry.read_set_signature.facts.is_empty(),
        "component_meta matrix slice: the constructed entry MUST \
         carry the `EffectiveExportSet` RouteSurface fact in its \
         `read_set_signature.facts`."
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&entry.read_set_signature.facts),
        "component_meta matrix slice: the permissive view MUST accept \
         an `EffectiveExportSet` RouteSurface fact in the entry's \
         `read_set_signature.facts`."
    );
}
