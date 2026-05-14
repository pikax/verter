//! Block 1.B matrix slice — `ComponentMetaResultEntry.fact_dep_signature`
//! must be able to carry `FactKey::Member` body facts so warm-hit
//! validation invalidates when a referenced member's BODY fingerprint
//! changes on a dep.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::component_meta_result_db::ComponentMetaResultEntry;
use verter_session::resolver_core::{FactVersionRef, ParseFactRef, PermissiveStoreView, StoreView};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
#[ignore = "block-1.b RED — closed by same-block implementation"]
fn component_meta_result_signature_carries_member() {
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
        window.contains("fact_dep_signature: Arc<[FactVersionRef]>")
            || window.contains("fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1.B matrix slice: ComponentMetaResultEntry must carry \
         `fact_dep_signature: Arc<[FactVersionRef]>`. Window:\n{window}"
    );

    let member_fact = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/src/types.ts".to_owned(),
        key: FactKey::Member {
            exporter: InternedName::from("Foo"),
            name: InternedName::from("a"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [9u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![member_fact].into_boxed_slice());

    let entry: ComponentMetaResultEntry<u32> = ComponentMetaResultEntry {
        payload: Arc::new(0u32),
        dep_signature: Arc::from(Vec::new().into_boxed_slice()),
        fact_dep_signature: Arc::clone(&signature),
    };
    assert!(
        !entry.fact_dep_signature.is_empty(),
        "Block 1.B matrix slice: the constructed entry must carry the \
         Member fact in its fact_dep_signature"
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&entry.fact_dep_signature),
        "permissive view must accept a Member fact in the entry's \
         fact_dep_signature"
    );
}
