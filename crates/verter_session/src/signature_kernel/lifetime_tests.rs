use super::lifetime::{SignatureStore, StoreError};
use super::read_view::SemanticReadView;
use super::records::{BinderSpace, SignatureSetRef};
use super::substitution::CallSubstitution;
use super::test_support::intern_one_call;
use crate::semantic_query::CanonicalTypeSubstitution;

#[test]
fn stale_epoch_handle_is_rejected() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(candidate) = set else {
        panic!("one");
    };
    let old = SemanticReadView::pin(&store);
    let new_epoch = store.replace_epoch().unwrap();
    assert_ne!(new_epoch, old.epoch());
    let fresh = SemanticReadView::pin(&store);
    assert_eq!(
        fresh.descriptor(candidate.signature).unwrap_err(),
        super::read_view::ReadError::StaleHandle
    );
    assert!(old.descriptor(candidate.signature).is_ok());
    let _ = StoreError::StaleHandle;
}

#[test]
fn old_pinned_reader_finishes_against_its_epoch() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let old = SemanticReadView::pin(&store);
    store.replace_epoch().unwrap();
    match old.read_set(set).unwrap() {
        super::read_view::BorrowedSet::One { .. } => {}
        _ => panic!("expected One, got a different borrowed set"),
    }
}

#[test]
fn unissued_handle_is_rejected() {
    let store = SignatureStore::new();
    let view = SemanticReadView::pin(&store);
    let bogus = super::records::SignatureDescriptorId::from_raw(0);
    assert_eq!(
        view.descriptor(bogus).unwrap_err(),
        super::read_view::ReadError::StaleHandle
    );
}

#[test]
fn live_readers_are_roots_until_drop() {
    let store = SignatureStore::new();
    assert_eq!(store.live_reader_count(), 0);
    let view = SemanticReadView::pin(&store);
    assert!(store.live_reader_count() >= 1);
    drop(view);
    assert_eq!(store.live_reader_count(), 0);
}

#[test]
fn retained_results_outlive_epoch_replacement_until_drained() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, c.signature);
    let result = super::records::AppliedResult::context_free(c.signature, subst, recipe);
    store.retain_result(result).unwrap();
    assert_eq!(store.retained_len(), 1);
    store.replace_epoch().unwrap();
    assert_eq!(store.retained_len(), 1);
    assert!(store.retained_descriptor(0).is_ok());
    store.drain_retained();
    assert_eq!(store.retained_len(), 0);
    assert_eq!(
        store.retained_descriptor(0).unwrap_err(),
        StoreError::Missing
    );
}

fn intern_recipe_of(
    store: &SignatureStore,
    _descriptor: super::records::SignatureDescriptorId,
) -> super::records::SignatureResultRecipeId {
    let locator = store.intern_body_locator(99, None).expect("locator");
    store
        .intern_recipe(
            super::records::SignatureResultRecipe::Body {
                return_obligation_key: super::records::ReturnObligationKey {
                    body_locator: locator,
                    evaluation: crate::semantic_query::ResultEvaluationContextId::from_raw(0),
                },
            },
            None,
        )
        .expect("recipe")
}

#[test]
fn binder_tokens_are_logical_and_reject_overflow() {
    let a = SignatureStore::binder_token(1, 0).unwrap();
    let b = SignatureStore::binder_token(1, 256).unwrap();
    let c = SignatureStore::binder_token(2, 0).unwrap();
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        SignatureStore::binder_token(1 << 31, 0).unwrap_err(),
        StoreError::InvalidBinderToken
    );
    let store_fwd = SignatureStore::new();
    let store_rev = SignatureStore::new();
    let s_fwd_1 = store_fwd
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let _s_fwd_2 = store_fwd
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let _s_rev_2 = store_rev
        .intern_binder_space(
            BinderSpace {
                key: 2,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let s_rev_1 = store_rev
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    assert_ne!(s_fwd_1.index(), s_rev_1.index());
    assert_eq!(
        store_fwd.binder_token_for(s_fwd_1, 0).unwrap(),
        store_rev.binder_token_for(s_rev_1, 0).unwrap()
    );
}

#[test]
fn intern_rejects_stale_embedded_handles_across_epoch_replacement() {
    let store = SignatureStore::new();
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    store.replace_epoch().unwrap();
    let layout = store
        .intern_layout(
            super::records::ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .unwrap();
    let err = store
        .intern_shape(
            super::records::SignatureInputShape {
                kind: super::records::SignatureKind::Call,
                binder_declarations: space,
                this_parameter: None,
                parameter_layout: layout,
                declared_minimum: 0,
                signature_semantic_flags: super::records::SignatureSemanticFlags::NONE,
            },
            None,
        )
        .unwrap_err();
    assert_eq!(err, StoreError::StaleHandle);
}

#[test]
fn lookup_result_does_not_publish_on_miss() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let SignatureSetRef::One(c) = set else {
        panic!("one");
    };
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let subst = store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap();
    let recipe = intern_recipe_of(&store, c.signature);
    let result = super::records::AppliedResult::context_free(c.signature, subst, recipe);
    assert_eq!(store.lookup_result(&result).unwrap(), None);
    let id = store.publish_result(result.clone(), None).unwrap();
    assert_eq!(store.lookup_result(&result).unwrap(), Some(id));
}

#[test]
fn concurrent_replace_epoch_publishes_in_order() {
    use std::sync::Arc;
    let store = Arc::new(SignatureStore::new());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let store = Arc::clone(&store);
            scope.spawn(move || store.replace_epoch().unwrap());
        }
    });
    assert_eq!(store.epoch().as_u32(), 9);
}

#[test]
fn compose_rejects_unrelated_codomain() {
    let store = SignatureStore::new();
    let decl = store
        .intern_binder_space(
            BinderSpace {
                key: 1,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let other = store
        .intern_binder_space(
            BinderSpace {
                key: 3,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let first = store
        .intern_substitution(
            CallSubstitution::map(
                decl,
                CanonicalTypeSubstitution::new(vec![(
                    SignatureStore::binder_token(1, 0).unwrap(),
                    SignatureStore::binder_token(1, 1).unwrap(),
                )]),
            ),
            None,
        )
        .unwrap();
    let second = store
        .intern_substitution(
            CallSubstitution::map(
                other,
                CanonicalTypeSubstitution::new(vec![(
                    SignatureStore::binder_token(3, 0).unwrap(),
                    crate::semantic_query::SemanticNodeId(9),
                )]),
            ),
            None,
        )
        .unwrap();
    assert_eq!(
        store.compose_after(first, second, None).unwrap_err(),
        StoreError::WrongBinderSpace
    );
}
