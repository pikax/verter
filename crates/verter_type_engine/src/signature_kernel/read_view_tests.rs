use super::lifetime::SignatureStore;
use super::read_view::{BorrowedSet, SemanticReadView};
use super::records::SignatureSetRef;
use super::test_support::{intern_one_call, WarmPositionalStore};

#[test]
fn warm_positional_one_does_not_take_a_shard_lock() {
    let fixture = WarmPositionalStore::fixture();
    let probe = fixture.lock_probe();
    assert_eq!(
        probe.acquires_after, probe.acquires_before,
        "warm positional read acquired intern-shard locks"
    );
}

#[test]
fn empty_read_allocates_no_candidate_table_access() {
    let store = SignatureStore::new();
    let view = SemanticReadView::pin(&store);
    let before = view.shard_lock_acquires();
    match view.read_set(SignatureSetRef::Empty).unwrap() {
        BorrowedSet::Empty => {}
        _ => panic!("expected Empty"),
    }
    assert_eq!(view.shard_lock_acquires(), before);
}

#[test]
fn warm_one_borrows_descriptor_and_provenance() {
    let store = SignatureStore::new();
    let set = intern_one_call(&store);
    let view = SemanticReadView::pin(&store);
    match view.read_set(set).unwrap() {
        BorrowedSet::One {
            candidate,
            descriptor,
            provenance,
        } => {
            assert_eq!(descriptor.template.epoch(), view.epoch());
            assert_eq!(provenance.overload_ordinal, 0);
            assert_eq!(candidate.signature.epoch(), view.epoch());
        }
        _ => panic!("expected One"),
    }
}
