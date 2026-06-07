//! Matrix slice: `fallthrough` × `import_ref`.
//!
//! Discrimination: `CachedFallthroughEntry.fact_versions` MUST be
//! able to carry `FactKey::ImportRef` facts so a fallthrough consumer
//! is invalidated when a barrel-retarget edit shifts the importer's
//! binding-to-export resolution.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::resolver_core::{FactVersionRef, ParseFactRef, PermissiveStoreView, StoreView};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn fallthrough_signature_carries_import_ref() {
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

    let import_fact = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/src/Comp.vue".to_owned(),
        key: FactKey::ImportRef {
            specifier: InternedSpecifier::from("./types"),
            binding: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [3u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![import_fact].into_boxed_slice());

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&signature),
        "fallthrough matrix slice: the permissive view MUST accept an \
         `ImportRef` fact in the Arc-stored fallthrough signature."
    );
}
