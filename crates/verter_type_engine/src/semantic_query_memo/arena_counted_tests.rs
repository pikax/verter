use super::*;
use crate::semantic_query::LiteralValue;

#[test]
fn arena_wait_counter_includes_hit_and_both_miss_acquisitions() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(30);
    const HOLD: Duration = Duration::from_millis(20);
    for (hit, acquisition) in [(true, 1), (false, 1), (false, 2)] {
        let mut arena = NodeArena::default();
        let provenance = Arc::new(crate::engine_provenance::EngineProvenance::default());
        arena.provenance = Some(Arc::clone(&provenance));
        let data = SemanticNodeData::Literal(LiteralValue::String("contended".into()));
        if hit {
            let _ = arena.push(data.clone());
        }
        let before = provenance
            .node_arena_inner_write_wait_ns
            .load(Ordering::Relaxed);
        let (parked_tx, parked_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let resume_rx = parking_lot::Mutex::new(resume_rx);
        let acquisitions = AtomicUsize::new(0);
        *arena.race_hook.lock() = Some(Arc::new(move |event| {
            if event == ArenaRaceEvent::BeforeInnerWrite
                && acquisitions.fetch_add(1, Ordering::Relaxed) + 1 == acquisition
            {
                parked_tx.send(()).unwrap();
                resume_rx.lock().recv_timeout(WAIT).unwrap();
            }
        }));
        std::thread::scope(|threads| {
            // Disconnect a parked worker before joining on assertion failure.
            let resume = resume_tx;
            let worker = threads.spawn(|| arena.push(data));
            parked_rx.recv_timeout(WAIT).unwrap();
            let held = arena.inner.write();
            resume.send(()).unwrap();
            std::thread::sleep(HOLD);
            drop(held);
            worker.join().unwrap();
        });
        *arena.race_hook.lock() = None;
        let waited = provenance
            .node_arena_inner_write_wait_ns
            .load(Ordering::Relaxed)
            - before;
        assert!(waited >= HOLD.as_nanos() as u64 / 2,
            "hit={hit}, acquisition={acquisition}: blocked write acquisition was not counted ({waited}ns)");
    }
}

#[test]
fn multiset_guard_distinguishes_planted_omission_and_duplicate() {
    let data = SemanticNodeData::TemplateLiteral {
        quasis: Arc::from([Arc::from(""), Arc::from(""), Arc::from("")]),
        expressions: crate::semantic_query::PayloadArray::from([
            SemanticNodeId(3),
            SemanticNodeId(3),
        ]),
    };
    assert_retained_walk_is_complete(&data);
    for planted in [smallvec::smallvec![3], smallvec::smallvec![3, 3, 3]] {
        assert!(std::panic::catch_unwind(|| assert_retained_multiset(&data, planted)).is_err());
    }
}

#[test]
fn counted_only_global_primitive_vocabulary_is_permanent() {
    use crate::semantic_query::PrimitiveKind::*;
    let arena = NodeArena::counted_for_tests();
    for kind in [
        String, Number, Boolean, Symbol, BigInt, Any, Unknown, Void, Never, Null, Undefined, Object,
    ] {
        drop(arena.intern_owned(SemanticNodeData::Primitive(kind), NodeScopeId::Global));
    }
    assert_eq!(arena.ownership_counts().permanent_nodes, 12);
    assert_eq!(arena.ownership_counts().roots, 0);
    drop(arena.intern_owned(
        SemanticNodeData::Opaque(crate::semantic_query::QueryError::Miss),
        NodeScopeId::Global,
    ));
    assert_eq!(arena.live_len(), 12);
}

#[test]
fn counted_stale_child_is_refused_before_allocating_parent() {
    let arena = NodeArena::counted_for_tests();
    let root = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("stale".into())),
        NodeScopeId::Global,
    );
    let id = root.id();
    drop(root);
    assert!(matches!(
        arena.try_intern_owned(SemanticNodeData::Alias(id), NodeScopeId::Global),
        Err(crate::semantic_query::QueryError::StaleSemanticOperand)
    ));
    assert_eq!(arena.live_len(), 0);
}

#[test]
fn counted_last_root_releases_repeated_internal_edges() {
    let arena = NodeArena::counted_for_tests();
    let child = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("child".into())),
        NodeScopeId::Global,
    );
    let parent = arena.intern_owned(
        SemanticNodeData::TemplateLiteral {
            quasis: Arc::from([Arc::from(""), Arc::from(""), Arc::from("")]),
            expressions: crate::semantic_query::PayloadArray::from([child.id(), child.id()]),
        },
        NodeScopeId::Global,
    );
    let child_id = child.id();
    assert_eq!(arena.owner_count(child_id), 3);
    drop(child);
    assert!(arena.is_live(child_id));
    drop(parent);
    assert!(!arena.is_live(child_id));
    assert_eq!(arena.live_len(), 0);
}

#[test]
fn counted_deep_dag_teardown_uses_a_worklist() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let arena = NodeArena::counted_for_tests();
            let mut root = arena.intern_owned(
                SemanticNodeData::Literal(LiteralValue::String("leaf".into())),
                NodeScopeId::Global,
            );
            for _ in 0..20_000 {
                root = arena.intern_owned(SemanticNodeData::Alias(root.id()), NodeScopeId::Global);
            }
            assert_eq!(arena.live_len(), 20_001);
            drop(root);
            assert_eq!(arena.live_len(), 0);
            assert!(arena
                .shards
                .iter()
                .all(|shard| shard.lock().index.is_empty()));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn counted_dedup_races_with_last_release_without_resurrection() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(30);
    for upgrade_wins in [true, false] {
        let arena = NodeArena::counted_for_tests();
        let data = SemanticNodeData::Literal(LiteralValue::String("rendezvous".into()));
        let original = arena.intern_owned(data.clone(), NodeScopeId::Global);
        let original_id = original.id();
        // A surviving payload Arc must not make a tombstoned weak candidate
        // eligible for dedup. Only the arena root count owns the slot.
        let payload = arena.get(original_id).unwrap();
        let (parked_tx, parked_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let resume_rx = parking_lot::Mutex::new(resume_rx);
        let (release_started_tx, release_started_rx) = mpsc::channel();
        let (released_tx, released_rx) = mpsc::channel();
        let parked = AtomicBool::new(false);
        let release_started = AtomicBool::new(false);
        let released = AtomicBool::new(false);
        let park_at = if upgrade_wins {
            ArenaRaceEvent::BeforeDedupRetain
        } else {
            ArenaRaceEvent::BeforeDedupLookup
        };
        *arena.race_hook.lock() = Some(Arc::new(move |event| {
            if event == park_at && !parked.swap(true, Ordering::SeqCst) {
                parked_tx.send(()).unwrap();
                resume_rx.lock().recv_timeout(WAIT).unwrap();
            }
            match event {
                ArenaRaceEvent::BeforeRootRelease { blocked }
                    if !release_started.swap(true, Ordering::SeqCst) =>
                {
                    release_started_tx.send(blocked).unwrap();
                }
                ArenaRaceEvent::AfterRootRelease if !released.swap(true, Ordering::SeqCst) => {
                    released_tx.send(()).unwrap();
                }
                _ => {}
            }
        }));
        let root = std::thread::scope(|threads| {
            // Disconnect the parked worker on assertion failure before
            // thread::scope joins it during unwinding.
            let resume = resume_tx;
            let lookup = threads.spawn(|| arena.intern_owned(data, NodeScopeId::Global));
            parked_rx.recv_timeout(WAIT).unwrap();
            let release = threads.spawn(move || drop(original));
            assert_eq!(
                release_started_rx.recv_timeout(WAIT).unwrap(),
                upgrade_wins,
                "live-slot validation and root acquisition must exclude final release"
            );
            if !upgrade_wins {
                // The release drops the arena lock before retiring the
                // weak index. Lookup still holds that shard, so it sees
                // the candidate after its slot has been tombstoned.
                released_rx.recv_timeout(WAIT).unwrap();
                assert!(!arena.is_live(original_id));
            }
            resume.send(()).unwrap();
            let root = lookup.join().unwrap();
            release.join().unwrap();
            root
        });
        *arena.race_hook.lock() = None;
        assert_eq!(root.id() == original_id, upgrade_wins);
        assert!(arena.is_live(root.id()));
        assert_eq!(arena.owner_count(root.id()), 1);
        assert_eq!(arena.live_len(), 1);
        drop(root);
        assert_eq!(arena.live_len(), 0);
        assert!(arena.inner.read().chunks.is_empty());
        assert!(arena
            .shards
            .iter()
            .all(|shard| shard.lock().index.is_empty()));
        drop(payload);
    }
}

#[test]
fn counted_concurrent_intern_and_drop_drains_all_slots() {
    let arena = Arc::new(NodeArena::counted_for_tests());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..2000 {
                    let root = arena.intern_owned(
                        SemanticNodeData::Literal(LiteralValue::String("racing".into())),
                        NodeScopeId::Global,
                    );
                    assert!(arena.is_live(root.id()));
                    let second =
                        arena.intern_owned(SemanticNodeData::Alias(root.id()), NodeScopeId::Global);
                    drop(root);
                    assert!(arena.is_live(second.id()));
                    drop(second);
                }
            });
        }
    });
    assert_eq!(arena.live_len(), 0);
    assert!(arena
        .shards
        .iter()
        .all(|shard| shard.lock().index.is_empty()));
}

#[test]
fn production_constructor_keeps_sweeps_authoritative_and_old_drops_safe() {
    let arena = NodeArena::default();
    assert!(!arena.counted());
    let root = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("closed".into())),
        NodeScopeId::File {
            canonical_id: Arc::from("/closed.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [7; 16],
            local_scope: None,
        },
    );
    assert_eq!(
        arena.release_canonical("/closed.ts", u64::MAX),
        vec![root.id()]
    );
    drop(root);
    assert_eq!(arena.live_len(), 0);
}

#[test]
#[should_panic(expected = "counted destruction cannot coexist with legacy sweeps")]
fn planted_counted_activation_with_legacy_sweep_is_rejected() {
    NodeArena::counted_for_tests().release_canonical("/closed.ts", u64::MAX);
}

/// Ids are never reused, so a long session allocates ever higher chunk
/// numbers. With one early node pinning its chunk, the chunk table must still
/// hold entries for the live chunks only, not for every chunk the session
/// ever allocated and released.
#[test]
fn chunk_table_storage_follows_live_chunks_not_allocation_history() {
    let arena = NodeArena::counted_for_tests();
    let pinned = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("pinned".into())),
        NodeScopeId::Global,
    );
    let churned_chunks = 64;
    for index in 0..churned_chunks * CHUNK_LEN {
        drop(arena.intern_owned(
            SemanticNodeData::Literal(LiteralValue::String(format!("churn{index}"))),
            NodeScopeId::Global,
        ));
    }
    assert!(arena.is_live(pinned.id()));
    let inner = arena.inner.read();
    let live_chunks = inner.chunks.len();
    assert!(live_chunks <= 2, "{live_chunks} chunks stayed live");
    assert!(
        inner.chunks.held_entries() <= 2 * live_chunks + CHUNK_WINDOW_SLACK,
        "{} table entries held for {live_chunks} live chunks after {churned_chunks} released ones",
        inner.chunks.held_entries()
    );
    assert_eq!(
        inner.chunks.keys().next(),
        Some(0),
        "the pinned chunk stays addressable"
    );
    drop(inner);
    assert_eq!(
        arena.get(pinned.id()).as_deref(),
        Some(&SemanticNodeData::Literal(LiteralValue::String(
            "pinned".into()
        )))
    );
}

/// A storage snapshot taken while an interner holds a shard and waits for
/// `inner` must not hold `inner` while it waits for that shard: both finish.
#[test]
fn storage_snapshot_completes_beside_an_interner_holding_a_shard() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_secs(30);
    let arena = Arc::new(NodeArena::counted_for_tests());
    let (parked_tx, parked_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    let resume_rx = parking_lot::Mutex::new(resume_rx);
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    let parked = AtomicBool::new(false);
    let snapshotting = AtomicBool::new(false);
    *arena.race_hook.lock() = Some(Arc::new(move |event| match event {
        ArenaRaceEvent::BeforeDedupLookup if !parked.swap(true, Ordering::SeqCst) => {
            parked_tx.send(()).unwrap();
            let _ = resume_rx.lock().recv_timeout(WAIT);
        }
        ArenaRaceEvent::BeforeShardStorageSnapshot
            if !snapshotting.swap(true, Ordering::SeqCst) =>
        {
            snapshot_tx.send(()).unwrap();
        }
        _ => {}
    }));
    // Detached workers: a regression deadlocks them, and the assertions
    // below then fail on their timeouts instead of hanging on a join.
    let (interned_tx, interned_rx) = mpsc::channel();
    let interner = Arc::clone(&arena);
    std::thread::spawn(move || {
        let root = interner.intern_owned(
            SemanticNodeData::Literal(LiteralValue::String("snapshot".into())),
            NodeScopeId::Global,
        );
        interned_tx.send(root.id()).unwrap();
    });
    parked_rx.recv_timeout(WAIT).unwrap();
    let (charged_tx, charged_rx) = mpsc::channel();
    let snapshot = Arc::clone(&arena);
    std::thread::spawn(move || {
        charged_tx.send(snapshot.storage_charge_bytes()).unwrap();
    });
    // The snapshot has read its slot figure and is about to wait for the
    // parked interner's shard; the interner now needs `inner`.
    snapshot_rx.recv_timeout(WAIT).unwrap();
    resume_tx.send(()).unwrap();
    let interned = interned_rx.recv_timeout(WAIT);
    let charged = charged_rx.recv_timeout(WAIT);
    *arena.race_hook.lock() = None;
    assert!(
        interned.is_ok(),
        "the interner acquires `inner` and finishes"
    );
    assert!(
        charged.is_ok(),
        "the snapshot acquires the shard and finishes"
    );
}
