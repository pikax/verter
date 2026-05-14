//! Block 1A matrix slice — `DerivedRawState.cached_resolved_meta`
//! (`ResolvedComponentMetaCacheEntry`) must carry
//! `FactKey::ImportRef` facts in its
//! `fact_versions: Arc<[FactVersionRef]>` signature.
//!
//! Pre-1A: wrapper holds `Vec<FactVersionRef>` and source-grep
//! FAILS. Post-1A: substrate is `Arc<[FactVersionRef]>` and an
//! `ImportRef` fact validates through the per-domain fast-path.

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
#[ignore = "block-1.a RED — closed by same-block implementation"]
fn cached_resolved_meta_signature_carries_import_ref() {
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
        "permissive view must accept an ImportRef fact in the Arc-stored signature"
    );
}
