//! Matrix slice — `ComponentMetaResultEntry.fact_dep_signature`
//! must be able to carry `FactKey::ImportRef` facts so warm-hit
//! validation invalidates when an importer's binding-to-export
//! resolution shifts (e.g. a barrel reexport flips target).

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
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

fn component_meta_result_signature_carries_import_ref() {
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
        "Matrix slice: ComponentMetaResultEntry must carry \
         `read_set_signature: ReadSetSignature`. Window:\n{window}"
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

    let entry: ComponentMetaResultEntry<u32> = ComponentMetaResultEntry {
        payload: Arc::new(0u32),
        read_set_signature: verter_session::for_tests::ReadSetSignature::new(Arc::clone(
            &signature,
        )),
        validated_at_generation: 0,
    };
    assert!(
        !entry.read_set_signature.facts.is_empty(),
        "Matrix slice: the constructed entry must carry the \
         ImportRef fact in its read_set_signature.facts"
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&entry.read_set_signature.facts),
        "permissive view must accept an ImportRef fact in the entry's \
         fact_dep_signature"
    );
}
