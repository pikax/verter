//! Block 1A matrix slice — `DerivedRawState.cached_resolved_meta`
//! (`ResolvedComponentMetaCacheEntry`) must carry
//! `FactKey::ModuleAugmentationIndexShape` facts in its
//! `fact_versions: Arc<[FactVersionRef]>` signature.
//!
//! Pre-1A: wrapper holds `Vec<FactVersionRef>` and source-grep
//! FAILS. Post-1A: substrate is `Arc<[FactVersionRef]>` and a
//! `ModuleAugmentationIndexShape` fact-kind validates through the
//! per-domain fast-path.

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{AugmentationTargetKindTag, InternedSpecifier};
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
fn cached_resolved_meta_signature_carries_module_aug_index_shape() {
    let src = read_session_src("types.rs");
    let needle = "pub(crate) struct ResolvedComponentMetaCacheEntry {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = src[idx..]
        .find("\n}")
        .expect("ResolvedComponentMetaCacheEntry struct close");
    let window = &src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1A matrix slice: ResolvedComponentMetaCacheEntry must carry \
         `fact_versions: Arc<[FactVersionRef]>` after the Block 1A migration. \
         Window:\n{window}"
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
        "permissive view must accept a ModuleAugmentationIndexShape fact in the \
         Arc-stored signature"
    );
}
