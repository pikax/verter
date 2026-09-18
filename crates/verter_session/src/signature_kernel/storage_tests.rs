use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use super::lifetime::{SignatureStore, StoreError};
use super::records::{
    BinderSpace, ParameterLayout, SignatureInputShape, SignatureKind, SignatureSemanticFlags,
};
use super::storage::InternError;
use super::test_support::intern_one_call;

fn intern_shape(store: &SignatureStore, min: u16) -> super::records::SignatureInputShapeId {
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let layout = store
        .intern_layout(
            ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .unwrap();
    store
        .intern_shape(
            SignatureInputShape {
                kind: SignatureKind::Call,
                binder_declarations: space,
                this_parameter: None,
                parameter_layout: layout,
                declared_minimum: min,
                signature_semantic_flags: SignatureSemanticFlags::NONE,
            },
            None,
        )
        .unwrap()
}

#[test]
fn duplicate_publisher_race_returns_one_id() {
    let store = Arc::new(SignatureStore::new());
    let space = store
        .intern_binder_space(
            BinderSpace {
                key: 0,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let layout = store
        .intern_layout(
            ParameterLayout {
                parameters: Box::from([]),
                rest: None,
            },
            None,
        )
        .unwrap();
    let shape = SignatureInputShape {
        kind: SignatureKind::Construct,
        binder_declarations: space,
        this_parameter: None,
        parameter_layout: layout,
        declared_minimum: 2,
        signature_semantic_flags: SignatureSemanticFlags::NONE,
    };
    let ids: Vec<_> = thread::scope(|scope| {
        let mut joins = Vec::new();
        // bounded-loop: one publisher per worker.
        for _ in 0..8 {
            let store = Arc::clone(&store);
            joins.push(scope.spawn(move || store.intern_shape(shape, None).unwrap()));
        }
        joins.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let first = ids[0];
    assert!(ids.iter().all(|id| *id == first));
}

#[test]
fn intern_order_does_not_change_logical_identity() {
    let forward = SignatureStore::new();
    let reverse = SignatureStore::new();
    let a_fwd = intern_shape(&forward, 1);
    let b_fwd = intern_shape(&forward, 2);
    let b_rev = intern_shape(&reverse, 2);
    let a_rev = intern_shape(&reverse, 1);
    assert_ne!(a_fwd, b_fwd);
    assert_ne!(b_fwd, b_rev);
    let space_fwd = forward
        .intern_binder_space(
            BinderSpace {
                key: 7,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    let space_rev = reverse
        .intern_binder_space(
            BinderSpace {
                key: 7,
                binders: Box::from([]),
            },
            None,
        )
        .unwrap();
    assert_ne!(a_fwd.index(), a_rev.index());
    assert_eq!(
        forward.binder_token_for(space_fwd, 0).unwrap(),
        reverse.binder_token_for(space_rev, 0).unwrap()
    );
    let one_f = intern_one_call(&forward);
    let one_r = intern_one_call(&reverse);
    let super::records::SignatureSetRef::One(cf) = one_f else {
        panic!();
    };
    let super::records::SignatureSetRef::One(cr) = one_r else {
        panic!();
    };
    let df = super::read_view::SemanticReadView::pin(&forward)
        .descriptor(cf.signature)
        .unwrap()
        .residual_binders;
    let dr = super::read_view::SemanticReadView::pin(&reverse)
        .descriptor(cr.signature)
        .unwrap()
        .residual_binders;
    assert_eq!(
        forward.binder_token_for(df, 0).unwrap(),
        reverse.binder_token_for(dr, 0).unwrap()
    );
}

#[test]
fn cancelled_producer_publishes_no_handle() {
    let store = SignatureStore::new();
    let cancelled = AtomicBool::new(true);
    let err = store.intern_body_locator(7, Some(&cancelled)).unwrap_err();
    assert_eq!(err, StoreError::Cancelled);
    let _ = InternError::Cancelled;
}

#[test]
fn panic_in_builder_publishes_no_handle() {
    let store = SignatureStore::new();
    let err = store
        .intern_with_shape(None, || panic!("builder"))
        .unwrap_err();
    assert_eq!(err, StoreError::Panicked);
    let ok = intern_shape(&store, 0);
    assert!(!ok.is_unissued());
}

#[test]
fn no_first_writer_state_on_shared_record() {
    let store = Arc::new(SignatureStore::new());
    let ids: Vec<_> = thread::scope(|scope| {
        let mut joins = Vec::new();
        // bounded-loop: concurrent identical publishers.
        for _ in 0..4 {
            let store = Arc::clone(&store);
            joins.push(scope.spawn(move || intern_one_call(&store)));
        }
        joins.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let first = ids[0];
    assert!(ids.iter().all(|id| *id == first));
    let a = intern_one_call(&store);
    let b = intern_one_call(&store);
    assert_eq!(a, b);
    assert_eq!(a, first);
}

#[test]
fn intern_overflow_publishes_no_handle() {
    use super::records::GraphEpoch;
    use super::storage::AppendInterner;
    let table: AppendInterner<u64> = AppendInterner::with_max_index(GraphEpoch::FIRST, 0);
    let _first = table.intern(1, None).unwrap();
    assert!(table.get(0).is_some());
    let err = table.intern(2, None).unwrap_err();
    assert_eq!(err, InternError::Overflow);
    assert!(table.lookup(&2).is_none());
}
