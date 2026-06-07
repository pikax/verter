//! Matrix slice — `DerivedRawState.cached_fallthrough`
//! (`CachedFallthroughEntry`) must carry `FactKey::ImportRef` facts
//! in its `fact_versions: Arc<[FactVersionRef]>` signature.
//!
//! The substrate is `Arc<[FactVersionRef]>` and an `ImportRef` fact
//! validates through the per-domain fast-path.

use std::sync::Arc;
use std::{fs, path};

use verter_semantic::facts::registry::{InternedName, InternedSpecifier, SymbolSpace};
use verter_semantic::facts::{FactKey, FactLane};
use verter_session::resolver_core::{
    FactVersionRef, ParseFactRef, PermissiveStoreView, StoreView, StoreViewCompatToken,
};

fn read_session_src(rel: &str) -> String {
    let p = path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    fs::read_to_string(&p).unwrap_or_else(|err| panic!("read {}: {err}", p.display()))
}

/// Non-permissive view that explicitly rejects the test's `ImportRef`
/// fact. Used to prove the `validates_fact_signature` dispatch on the
/// Arc-stored substrate makes a real validator decision — a fabricated
/// signature combined with a non-permissive view discriminates the
/// per-domain validator wiring from the permissive identity case.
struct RejectImportRefView;

impl StoreView for RejectImportRefView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
        }
    }

    fn validates(&self, fact: &FactVersionRef) -> bool {
        !matches!(
            fact,
            FactVersionRef::Parse(ParseFactRef {
                key: FactKey::ImportRef { .. },
                ..
            })
        )
    }
}

#[test]
fn cached_fallthrough_signature_carries_import_ref() {
    let src = read_session_src("types.rs");
    let needle = "pub(crate) struct CachedFallthroughEntry {";
    let idx = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in types.rs"));
    let end = src[idx..]
        .find("\n}")
        .expect("CachedFallthroughEntry struct close");
    let window = &src[idx..idx + end];
    assert!(
        window.contains("fact_versions: Arc<[crate::resolver_core::FactVersionRef]>"),
        "matrix slice: CachedFallthroughEntry must carry \
         `fact_versions: Arc<[FactVersionRef]>`. \
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

    // Positive: a permissive view accepts the ImportRef in the
    // Arc-stored signature — this proves the dispatch path
    // structurally admits the fact-kind.
    let permissive = PermissiveStoreView;
    assert!(
        permissive.validates_fact_signature(&signature),
        "permissive view must accept an ImportRef fact in the Arc-stored signature"
    );

    // Discriminator: a view that explicitly
    // rejects ImportRef facts must cause `validates_fact_signature`
    // to return false. Without this assertion the permissive view
    // alone is non-discriminating — a regression that breaks the
    // dispatch's short-circuit semantics or stops walking the
    // signature would still pass the permissive arm because
    // PermissiveStoreView.validates returns true unconditionally.
    let rejecting = RejectImportRefView;
    assert!(
        !rejecting.validates_fact_signature(&signature),
        "`validates_fact_signature` must propagate a per-fact \
         rejection on the Arc-stored substrate. A view that rejects \
         ImportRef must turn the whole-signature decision negative; \
         otherwise the dispatch is not actually consulting per-fact \
         validators on this fact-kind."
    );
}
