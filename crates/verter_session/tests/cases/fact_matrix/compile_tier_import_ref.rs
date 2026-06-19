//! Matrix slice: `compile_tier` × `import_ref`.
//!
//! Discrimination: `CompileSlot.fact_dep_signature` MUST be the
//! `ReadSetSignature` carrier and able to carry `FactKey::ImportRef`
//! facts so a consumer importing a type-only binding from a barrel
//! that retargets is invalidated on the next warm read.

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
fn compile_tier_signature_carries_import_ref() {
    let src = read_session_src("types.rs");
    assert!(
        src.contains("fact_dep_signature: crate::fact_signature_helpers::ReadSetSignature"),
        "compile_tier matrix slice: `CompileSlot` MUST carry \
         `fact_dep_signature: ReadSetSignature` (the carrier that wraps \
         `Arc<[FactVersionRef]>` + the overflow flag). A regression that \
         drops the typed carrier would bypass per-domain fact validation."
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
        "compile_tier matrix slice: the permissive view MUST accept \
         an `ImportRef` fact in the Arc-stored compile-slot signature \
         so a barrel-retarget edit invalidates the consumer warm hit."
    );
}
