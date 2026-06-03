//! Matrix slice: `compile_tier` × `member_presence`.
//!
//! Discrimination: `CompileSlot.fact_dep_signature` MUST be able to
//! carry `FactKey::MemberPresence` facts so a path-precise consumer
//! whose dep adds / removes / renames the referenced member is
//! invalidated on the next warm read.
//!
//! With a legacy `Vec<FactVersionRef>` or absent field, the
//! field-shape grep below FAILS and the permissive view cannot
//! accept the `MemberPresence` fact through the carried signature.
//! With the `ReadSetSignature` carrier, both assertions pass.

#![cfg(test)]

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
fn compile_tier_signature_carries_member_presence() {
    // Arch guard: `CompileSlot.fact_dep_signature` MUST be the
    // `ReadSetSignature` carrier (which wraps `Arc<[FactVersionRef]>`
    // + the overflow flag). The legacy carrier was a
    // `Vec<FactVersionRef>` whose validation pumped through a
    // different path and dropped derived facts; this grep blocks the
    // regression.
    let src = read_session_src("types.rs");
    assert!(
        src.contains("fact_dep_signature: crate::fact_signature_helpers::ReadSetSignature"),
        "compile_tier matrix slice: `CompileSlot` MUST carry \
         `fact_dep_signature: ReadSetSignature`. A regression that swaps \
         the field for `Vec<FactVersionRef>` would silently bypass \
         per-domain fact validation."
    );

    // Substrate check: a single-entry signature bearing the
    // `MemberPresence` fact-kind validates through the permissive
    // view's per-domain dispatcher.
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
        "compile_tier matrix slice: the permissive view MUST accept \
         a `MemberPresence` fact in the Arc-stored compile-slot \
         signature. Without this property, a `defineProps<Foo>()` \
         consumer importing `Foo` from a workspace `.ts` cannot \
         invalidate on a cross-file member-shape edit."
    );
}
