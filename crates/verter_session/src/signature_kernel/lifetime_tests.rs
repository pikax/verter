use super::lifetime::{SignatureStore, StoreError};
use super::read_view::SemanticReadView;
use super::records::SignatureSetRef;
use super::test_support::intern_one_call;

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
    let result = super::records::AppliedResult::context_free(
        c.signature,
        super::records::CallSubstitutionId::from_raw(0),
        super::records::SignatureResultRecipeId::from_raw(0),
    );
    store.retain_result(result);
    assert_eq!(store.retained_len(), 1);
    store.replace_epoch().unwrap();
    assert_eq!(store.retained_len(), 1);
    store.drain_retained();
    assert_eq!(store.retained_len(), 0);
}
