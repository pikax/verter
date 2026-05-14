//! Block 1A matrix slice — `DerivedRawState.cached_meta_payload`
//! must carry `FactKey::MemberPresence` facts in its
//! `fact_versions: Arc<[FactVersionRef]>` signature so warm-hit
//! validation invalidates when a referenced member's presence
//! (existence) flips on a dep.
//!
//! Pre-1A: the wrapper's field shape is `Vec<FactVersionRef>` and
//! the source-grep arch guard FAILS because `Arc<[...]>` is not
//! present. Post-1A: substrate carries `Arc<[FactVersionRef]>` and
//! the synthetic single-entry `MemberPresence` signature validates
//! through the per-domain fast-path.
//!
//! The behavioural producer + cold-meta tracer participation is
//! covered by the positive fact-validation test and by the broader
//! Family A / Family B suites; this slice fixes the structural
//! contract that the `cached_meta_payload` substrate accepts a
//! `MemberPresence` fact-kind by construction.

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
fn cached_meta_payload_signature_carries_member_presence() {
    // Structural arch guard: `CachedMetaPayload.fact_versions` must
    // be `Arc<[FactVersionRef]>` so the slice can hold any
    // `FactVersionRef` variant — including a `MemberPresence` fact.
    let src = read_session_src("types.rs");
    let needle = "pub(crate) struct CachedMetaPayload {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let window_end = src[idx..]
        .find("\n}")
        .expect("CachedMetaPayload struct close");
    let window = &src[idx..idx + window_end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "Block 1A matrix slice: CachedMetaPayload must carry \
         `fact_versions: Arc<[FactVersionRef]>` after the Block 1A migration. \
         Window:\n{window}"
    );

    // Behavioural slice: a single `MemberPresence` entry placed in
    // an `Arc<[FactVersionRef]>` validates true through the per-
    // domain dispatcher under a permissive view. This proves the
    // Block 1A substrate is structurally able to carry a
    // MemberPresence fact-kind.
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
