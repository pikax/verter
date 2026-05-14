//! Block 1.B matrix slice — `ComponentMetaResultEntry.fact_dep_signature`
//! must be able to carry `FactKey::MemberPresence` facts so warm-hit
//! validation invalidates when a referenced member is renamed /
//! added / removed on a dep (path-precise per R28).
//!
//! Pre-1.B: the entry only carried `dep_signature: DepSignature`
//! (whole-hash + project-generation pairs); the field
//! `fact_dep_signature` does not exist, so this test fails to
//! compile.
//!
//! Post-1.B: the entry carries
//! `fact_dep_signature: Arc<[FactVersionRef]>` and the
//! `MemberPresence` fact-kind validates through the per-domain
//! fast-path under a permissive view. The producer-side wiring is
//! covered by the behavioural tests; this slice asserts the
//! substrate can hold the matrix variant.

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

fn component_meta_result_signature_carries_member_presence() {
    // Structural arch guard: `ComponentMetaResultEntry` must declare
    // `fact_dep_signature: Arc<[FactVersionRef]>` after Block 1.B.
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

    // Compile-time witness: build a `MemberPresence`-bearing
    // signature and verify the substrate accepts it through the
    // per-domain dispatcher under a permissive view. Constructing
    // an entry requires both the legacy `dep_signature` and the
    // new `fact_dep_signature` field — the type-checker enforces
    // the structural contract.
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
        dep_signature: Arc::from(Vec::new().into_boxed_slice()),
        fact_dep_signature: Arc::clone(&signature),
    };
    assert!(
        !entry.fact_dep_signature.is_empty(),
        "Block 1.B matrix slice: the constructed entry must carry the \
         MemberPresence fact in its fact_dep_signature"
    );

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&entry.fact_dep_signature),
        "permissive view must accept the MemberPresence fact in the \
         entry's fact_dep_signature"
    );
}
