//! Matrix slice: `compile_tier` × `member`.
//!
//! Discrimination: `CompileSlot.fact_dep_signature` MUST be the
//! `ReadSetSignature` carrier and able to carry `FactKey::Member`
//! body facts so a consumer whose dep edits a referenced member's
//! BODY fingerprint is invalidated on the next warm read.

#![cfg(test)]

use std::sync::Arc;
use std::{fs, path};

use verter_session::resolver_core::PermissiveStoreView;
use verter_session_query::facts::fact_cache::{FactVersionRef, ParseFactRef};
use verter_session_query::facts::registry::{InternedName, SymbolSpace};
use verter_session_query::facts::store_view::StoreView;
use verter_session_query::facts::{FactKey, FactLane};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

#[test]
fn compile_tier_signature_carries_member() {
    let src = read_session_src("types.rs");
    assert!(
        src.contains(
            "fact_dep_signature: verter_session_query::facts::fact_cache::ReadSetSignature"
        ),
        "compile_tier matrix slice: `CompileSlot` MUST carry \
         `fact_dep_signature: ReadSetSignature` (the carrier that wraps \
         `Arc<[FactVersionRef]>`). A regression that \
         drops the typed carrier would bypass per-domain fact validation."
    );

    let member_fact = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/src/types.ts".to_owned(),
        key: FactKey::Member {
            exporter: InternedName::from("Foo"),
            name: verter_type_expr::facts::FactPropertyKey::identifier("a"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [9u8; 16],
    });
    let signature: Arc<[FactVersionRef]> = Arc::from(vec![member_fact].into_boxed_slice());

    let view = PermissiveStoreView;
    assert!(
        view.validates_fact_signature(&signature),
        "compile_tier matrix slice: the permissive view MUST accept \
         a `Member` fact in the Arc-stored compile-slot signature so \
         a body-fingerprint edit on a referenced member invalidates \
         the consumer warm hit."
    );
}
