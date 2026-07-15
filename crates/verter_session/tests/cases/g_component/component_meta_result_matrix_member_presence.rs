//! `ComponentMetaResultEntry.read_set_signature.facts` must be able
//! to carry `FactKey::MemberPresence` facts so warm-hit validation
//! invalidates when a referenced member is renamed / added / removed
//! on a dep (path-precise per R28).
//!
//! After the carrier consolidation: the entry stores
//! `read_set_signature: ReadSetSignature` where the `facts: Arc<[FactVersionRef]>`
//! rail validates through the per-domain fast-path under a permissive
//! view. The producer-side wiring is covered by the behavioural tests;
//! this slice asserts the substrate can hold the matrix variant.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::component_meta_result_db::ComponentMetaResultEntry;
use verter_session::for_tests::ReadSetSignature;
use verter_session::resolver_core::{FactVersionRef, ParseFactRef, PermissiveStoreView, StoreView};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]

fn component_meta_result_signature_carries_member_presence() {
    // Structural arch guard: `ComponentMetaResultEntry` must declare
    // `read_set_signature: ReadSetSignature` after the carrier
    // consolidation. The carrier's `facts: Arc<[FactVersionRef]>` rail
    // carries the path-precise fact signature; the legacy rail carries
    // the whole-hash / project-generation pairs.
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
        "ComponentMetaResultEntry must carry \
         `read_set_signature: ReadSetSignature`. Window:\n{window}"
    );

    // Compile-time witness: build a `MemberPresence`-bearing
    // signature and verify the substrate accepts it through the
    // per-domain dispatcher under a permissive view. Constructing
    // an entry requires the carrier's `facts` rail to be populated.
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

    let entry: ComponentMetaResultEntry<u32> = ComponentMetaResultEntry {
        payload: Arc::new(0u32),
        read_set_signature: ReadSetSignature::new(Arc::clone(&signature)),
        validated_at_generation: 0,
    };
    assert!(
        !entry.read_set_signature.facts.is_empty(),
        "the constructed entry must carry the MemberPresence fact in \
         its `read_set_signature.facts`"
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&entry.read_set_signature.facts),
        "permissive view must accept the MemberPresence fact in the \
         entry's `read_set_signature.facts`"
    );
}
