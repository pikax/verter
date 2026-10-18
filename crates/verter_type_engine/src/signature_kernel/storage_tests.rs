use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use super::lifetime::{SignatureStore, StoreError};
use super::records::{
    BinderSpace, ParameterLayout, SignatureInputShape, SignatureKind, SignatureSemanticFlags,
};
use super::storage::InternError;
use super::test_support::intern_one_call;

fn intern_shape(store: &SignatureStore, min: usize) -> super::records::SignatureInputShapeId {
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
fn declared_minimum_preserves_counts_above_u16() {
    let store = SignatureStore::new();
    let shape = intern_shape(&store, usize::from(u16::MAX) + 1);
    let view = super::read_view::SemanticReadView::pin(&store);
    assert_eq!(view.shape(shape).unwrap().declared_minimum, 65_536);
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

#[test]
fn overflow_race_does_not_plant_unmapped_slots() {
    use super::records::GraphEpoch;
    use super::storage::AppendInterner;
    let table = Arc::new(AppendInterner::<u64>::with_max_index(GraphEpoch::FIRST, 3));
    let results: Vec<_> = thread::scope(|scope| {
        let mut joins = Vec::new();
        // bounded-loop: distinct publishers racing the overflow bound.
        for v in 0..32u64 {
            let table = Arc::clone(&table);
            joins.push(scope.spawn(move || table.intern(v, None)));
        }
        joins.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let oks: Vec<u64> = results
        .iter()
        .filter_map(|r| r.as_ref().ok().copied())
        .collect();
    assert!(
        oks.len() <= 4,
        "published more than max_index+1 handles: {}",
        oks.len()
    );
    assert_eq!(oks.len(), {
        let mut uniq = oks.clone();
        uniq.sort_unstable();
        uniq.dedup();
        uniq.len()
    });
    for v in 0..32u64 {
        match table.lookup(&v) {
            Some(handle) => assert!(oks.contains(&handle), "lookup published unmapped {v}"),
            None => assert!(
                results
                    .iter()
                    .any(|r| r.as_ref().err() == Some(&InternError::Overflow)),
                "missing value {v} was not overflow"
            ),
        }
    }
    assert!(
        table.get(4).is_none(),
        "overflow race planted a slot past max_index"
    );
}

/// Boxcar 0.2.14 opens bucket `b` (positions `[2^b, 2^(b+1))`, index `i`
/// at position `i + 32`) by its first write, and opens bucket `b + 1`
/// eagerly once a write lands on entry `2^b - 2^b / 8`. The bytes a table
/// charges for its first `pushes` writes are therefore every bucket up to
/// the last write's, plus the next one once its eager point is reached.
#[test]
fn an_epoch_table_charges_every_boxcar_bucket_once_including_the_eager_next_one() {
    use super::records::GraphEpoch;
    use super::storage::AppendInterner;
    use verter_session_query::retention::allocation::heap_block_bytes;

    let bucket =
        |b: u32| heap_block_bytes((1_usize << b) * std::mem::size_of::<(u64, AtomicBool)>());
    let expected = |pushes: usize| {
        let last = pushes - 1 + 32;
        let top = last.ilog2();
        let eager = last - (1 << top) >= (1 << top) - ((1 << top) >> 3);
        (5..=top).map(bucket).sum::<usize>() + if eager { bucket(top + 1) } else { 0 }
    };
    for pushes in [1, 28, 29, 88, 89, 90, 96, 97, 1_000] {
        let table = AppendInterner::<u64>::new(GraphEpoch::FIRST);
        let charged = (0..pushes)
            .map(|index| table.opened_bucket_bytes(index))
            .sum::<usize>();
        assert_eq!(charged, expected(pushes), "{pushes} writes");
    }
    // Index 88 is bucket 6's eager point: bucket 7 exists before any write
    // reaches position 128.
    let table = AppendInterner::<u64>::new(GraphEpoch::FIRST);
    let before = (0..88)
        .map(|index| table.opened_bucket_bytes(index))
        .sum::<usize>();
    assert_eq!(table.opened_bucket_bytes(88), bucket(7));
    assert_eq!(before, bucket(5) + bucket(6));
}

/// A settlement that read an older, smaller live total must not publish a
/// charge below the total once growth that fitted the previous charge
/// skipped its own settlement meanwhile.
#[test]
fn a_shrinking_settlement_racing_lock_free_growth_still_covers_the_live_total() {
    use super::storage::EpochStorage;
    use std::sync::atomic::Ordering;
    use std::sync::{mpsc, OnceLock};
    use std::time::Duration;
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

    const GRANULE: usize = 64 * 1024;
    const WAIT: Duration = Duration::from_secs(30);
    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    let storage = EpochStorage::new(Arc::new(OnceLock::from(Arc::clone(&account))));
    storage.add(5 * GRANULE);
    assert_eq!(storage.charged(), 5 * GRANULE);

    let (parked_tx, parked_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    let resume_rx = parking_lot::Mutex::new(resume_rx);
    let parked = AtomicBool::new(false);
    storage.set_settle_hook(Some(Arc::new(move || {
        if !parked.swap(true, Ordering::SeqCst) {
            parked_tx.send(()).unwrap();
            let _ = resume_rx.lock().recv_timeout(WAIT);
        }
    })));
    thread::scope(|scope| {
        let resume = resume_tx;
        let shrink = scope.spawn(|| storage.sub(4 * GRANULE));
        // The settlement read a live total of one granule and will shrink
        // the charge to it.
        parked_rx.recv_timeout(WAIT).unwrap();
        // This growth fits the still-published five-granule charge, so it
        // settles nothing itself.
        storage.add(3 * GRANULE);
        resume.send(()).unwrap();
        shrink.join().unwrap();
    });
    storage.set_settle_hook(None);
    assert_eq!(storage.used(), 4 * GRANULE);
    assert!(
        storage.charged() >= storage.used(),
        "charged {} < live {}",
        storage.charged(),
        storage.used()
    );
    assert_eq!(account.snapshot().pinned_bytes, storage.charged());
}
