//! Block 1A matrix slice — `DerivedRawState.cached_resolved_meta`
//! (`ResolvedComponentMetaCacheEntry`) must carry
//! `FactKey::MemberPresence` facts in its
//! `fact_versions: Arc<[FactVersionRef]>` signature.
//!
//! Pre-1A: wrapper holds `Vec<FactVersionRef>` and source-grep
//! FAILS. Post-1A: substrate is `Arc<[FactVersionRef]>` and a
//! `MemberPresence` fact validates through the per-domain fast-
//! path.

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
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
fn cached_resolved_meta_signature_carries_member_presence() {
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

    let presence_fact = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/src/types.ts".to_owned(),
        key: FactKey::MemberPresence {
            exporter: InternedName::from("Foo"),
            name: InternedName::from("a"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [7u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![presence_fact].into_boxed_slice());
    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&signature),
        "permissive view must accept the MemberPresence fact in the Arc-stored signature"
    );
}
