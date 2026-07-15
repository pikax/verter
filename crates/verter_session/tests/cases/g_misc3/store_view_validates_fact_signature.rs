//! RED test: `StoreView::validates_fact_signature` default impl correctly
//! delegates to `validates` for each fact in the slice.
//!
//! A `PermissiveStoreView` (validates everything) must return `true` for any
//! non-empty signature. A custom always-rejecting view must return `false` on
//! the first non-matching fact.

use verter_session::resolver_core::{
    FactVersionRef, PermissiveStoreView, StoreView, StoreViewCompatToken,
};

fn test_fact(n: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("validate_test_{n}.ts"),
        hash: [n; 16],
    }
}

// A StoreView that always rejects every fact.
struct RejectAllView;

impl StoreView for RejectAllView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        }
    }

    fn validates(&self, _fact: &FactVersionRef) -> bool {
        false
    }
}

#[test]
fn permissive_view_validates_any_signature() {
    let view = PermissiveStoreView;
    let sig: Vec<FactVersionRef> = (0..5).map(test_fact).collect();

    // validates_fact_signature must return true for a permissive view.
    assert!(
        view.validates_fact_signature(&sig),
        "PermissiveStoreView must validate any non-empty signature"
    );
}

#[test]
fn permissive_view_validates_empty_signature() {
    let view = PermissiveStoreView;
    // Empty slice: trivially true.
    assert!(
        view.validates_fact_signature(&[]),
        "validates_fact_signature on empty slice must return true"
    );
}

#[test]
fn reject_all_view_fails_on_single_fact() {
    let view = RejectAllView;
    let sig = vec![test_fact(99)];

    // A view that rejects everything must return false on the first entry.
    assert!(
        !view.validates_fact_signature(&sig),
        "RejectAllView must return false for any non-empty signature"
    );
}

#[test]
fn reject_all_view_empty_signature_is_true() {
    let view = RejectAllView;
    // Empty slice always returns true regardless of view (trivially valid).
    assert!(
        view.validates_fact_signature(&[]),
        "validates_fact_signature on empty slice must always be true"
    );
}

#[test]
fn validates_fact_signature_short_circuits_on_first_failure() {
    // A view that only accepts the first fact and rejects all others.
    struct FirstOnlyView(FactVersionRef);

    impl StoreView for FirstOnlyView {
        fn compat_token(&self) -> StoreViewCompatToken {
            StoreViewCompatToken {
                epoch: 0,
                session: None,
                validity_fingerprint: 0,
            }
        }

        fn validates(&self, fact: &FactVersionRef) -> bool {
            fact == &self.0
        }
    }

    let first = test_fact(0);
    let second = test_fact(1);
    let view = FirstOnlyView(first.clone());

    // Signature with only the first fact: should pass.
    assert!(view.validates_fact_signature(std::slice::from_ref(&first)));
    // Signature with first + second: second fails, whole sig fails.
    assert!(!view.validates_fact_signature(&[first, second]));
}
