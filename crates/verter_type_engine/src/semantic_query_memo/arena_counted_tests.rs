use super::*;
use crate::semantic_query::LiteralValue;

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(feature = "semantic-observe")]
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
        // The hit case's seed stays owned across the worker's intern, so
        // the worker finds it live rather than interning a fresh node.
        let seeded = hit.then(|| arena.intern_owned(data.clone(), NodeScopeId::Global));
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
            let interned = worker.join().unwrap();
            if let Some(seeded) = &seeded {
                assert_eq!(interned, seeded.id(), "the worker hit the live seed");
            }
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
        quasis: verter_type_expr::OwnedSlice::<verter_type_expr::OwnedStr>::from([
            verter_type_expr::OwnedStr::from(""),
            verter_type_expr::OwnedStr::from(""),
            verter_type_expr::OwnedStr::from(""),
        ]),
        expressions: crate::semantic_query::PayloadArray::from([
            SemanticNodeId(3),
            SemanticNodeId(3),
        ]),
    };
    assert_retained_walk_is_complete(&data);
    for planted in [smallvec::smallvec![3], smallvec::smallvec![3, 3, 3]] {
        assert!(std::panic::catch_unwind(|| assert_retained_multiset(&data, planted)).is_err());
    }
    // A walk that visits the same operands in another order retains the
    // same multiset; a substituted operand does not.
    let distinct = SemanticNodeData::TemplateLiteral {
        quasis: verter_type_expr::OwnedSlice::<verter_type_expr::OwnedStr>::from([
            verter_type_expr::OwnedStr::from(""),
            verter_type_expr::OwnedStr::from(""),
            verter_type_expr::OwnedStr::from(""),
        ]),
        expressions: crate::semantic_query::PayloadArray::from([
            SemanticNodeId(3),
            SemanticNodeId(4),
        ]),
    };
    assert_retained_multiset(&distinct, smallvec::smallvec![4, 3]);
    assert!(std::panic::catch_unwind(|| assert_retained_multiset(
        &distinct,
        smallvec::smallvec![3, 5]
    ))
    .is_err());
}

#[test]
fn counted_only_global_primitive_vocabulary_is_permanent() {
    use crate::semantic_query::PrimitiveKind::*;
    let arena = NodeArena::default();
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

/// Releasing the vocabulary hold destroys the vocabulary nobody holds,
/// leaves a held vocabulary node and every other owned node readable, and
/// an intern afterwards re-establishes permanence — on a fresh node and on
/// the held one alike.
#[test]
fn released_vocabulary_drains_unheld_and_keeps_held_nodes() {
    use crate::semantic_query::PrimitiveKind::*;
    let arena = NodeArena::default();
    drop(arena.intern_owned(SemanticNodeData::Primitive(String), NodeScopeId::Global));
    let unheld = arena.intern_owned(SemanticNodeData::Primitive(Boolean), NodeScopeId::Global);
    let unheld_id = unheld.id();
    drop(unheld);
    let held = arena.intern_owned(SemanticNodeData::Primitive(Number), NodeScopeId::Global);
    let literal = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("kept".into())),
        NodeScopeId::Global,
    );
    assert_eq!(arena.ownership_counts().permanent_nodes, 3);
    assert_eq!(arena.live_len(), 4);

    assert_eq!(arena.release_vocabulary(), 3);
    assert_eq!(arena.ownership_counts().permanent_nodes, 0);
    assert_eq!(arena.live_len(), 2, "only the held nodes survive");
    assert!(arena.live_payload(unheld_id).is_none());
    assert!(matches!(
        arena.live_payload(held.id()).as_deref(),
        Some(SemanticNodeData::Primitive(Number))
    ));
    assert!(arena.live_payload(literal.id()).is_some());
    assert_eq!(
        arena.release_vocabulary(),
        0,
        "nothing is permanent any more"
    );

    // A fresh intern of released vocabulary is a new permanent node.
    let again = arena.intern_owned(SemanticNodeData::Primitive(Boolean), NodeScopeId::Global);
    assert_ne!(again.id(), unheld_id, "ids are never reused");
    drop(again);
    assert_eq!(arena.ownership_counts().permanent_nodes, 1);
    // An intern finding the held node makes it permanent again, so it
    // outlives its holder; without the intern it dies with the holder.
    let held_id = held.id();
    drop(arena.intern_owned(SemanticNodeData::Primitive(Number), NodeScopeId::Global));
    drop(held);
    assert!(arena.live_payload(held_id).is_some());
    assert_eq!(arena.ownership_counts().permanent_nodes, 2);
    assert_eq!(arena.release_vocabulary(), 2);
    drop(literal);
    assert_eq!(arena.live_len(), 0);
}

#[test]
fn counted_stale_child_is_refused_before_allocating_parent() {
    let arena = NodeArena::default();
    let root = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("stale".into())),
        NodeScopeId::Global,
    );
    let id = root.id();
    drop(root);
    assert!(matches!(
        arena.try_intern_owned::<verter_audit::observe::Uncaptured>(
            &crate::request_observers::CaptureTargets::EMPTY,
            SemanticNodeData::Alias(id),
            NodeScopeId::Global
        ),
        Err(crate::semantic_query::QueryError::StaleSemanticOperand)
    ));
    assert_eq!(arena.live_len(), 0);
}

#[test]
fn counted_last_root_releases_repeated_internal_edges() {
    let arena = NodeArena::default();
    let child = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("child".into())),
        NodeScopeId::Global,
    );
    let parent = arena.intern_owned(
        SemanticNodeData::TemplateLiteral {
            quasis: verter_type_expr::OwnedSlice::<verter_type_expr::OwnedStr>::from([
                verter_type_expr::OwnedStr::from(""),
                verter_type_expr::OwnedStr::from(""),
                verter_type_expr::OwnedStr::from(""),
            ]),
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
            let arena = NodeArena::default();
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
        let arena = NodeArena::default();
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
    let arena = Arc::new(NodeArena::default());
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

/// The production constructor is the counted regime: the last release of a
/// root destroys the node and the children it alone kept, and nothing else
/// can. Discriminating against an inactive production mode (a last release
/// that leaves the slot live for a later sweep) and against a destroyed slot
/// that still reads its payload.
#[test]
fn production_constructor_destroys_a_node_on_its_last_release() {
    let arena = NodeArena::default();
    let child = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("child".into())),
        NodeScopeId::Global,
    );
    let parent = arena.intern_owned(
        SemanticNodeData::Array {
            element: child.id(),
            readonly: false,
        },
        NodeScopeId::Global,
    );
    let (child_id, parent_id) = (child.id(), parent.id());
    drop(child);
    assert!(
        arena.is_live(child_id),
        "the parent's counted edge keeps the child"
    );
    drop(parent);
    assert!(!arena.is_live(parent_id));
    assert!(
        !arena.is_live(child_id),
        "the parent's destruction releases its edge"
    );
    assert_eq!(arena.live_len(), 0);
    assert!(NodeData::ptr_eq(
        &arena.get(child_id).unwrap(),
        &arena.released_placeholder
    ));
    assert_eq!(
        super::super::node_roots::NodeRootSet::acquire(
            Arc::clone(&arena.core),
            smallvec::smallvec![child_id]
        )
        .err(),
        Some(crate::semantic_query::QueryError::StaleSemanticOperand),
        "a destroyed ordinal can never be rooted again"
    );
}

/// Only slots a sidecar describes are logged when destroyed, so the pending
/// log never outgrows the stale sidecar entries draining it removes; a slot
/// marked after its destruction is refused, so its writer removes the entry
/// itself. Discriminating against logging every destruction (the log would
/// hold both ids) and against accepting a mark on a destroyed slot.
#[test]
fn only_sidecar_described_slots_enter_the_destroyed_log() {
    let arena = NodeArena::default();
    let account = verter_session_query::retention::SemanticRetentionAccount::new(
        verter_session_query::retention::RetentionLimits::defaults(),
    );
    arena.core.bind_retention_account(&account);
    let described = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("described".into())),
        NodeScopeId::Global,
    );
    let plain = arena.intern_owned(
        SemanticNodeData::Literal(LiteralValue::String("plain".into())),
        NodeScopeId::Global,
    );
    let (described_id, plain_id) = (described.id(), plain.id());
    assert!(arena.track_sidecar(described_id));
    drop(described);
    drop(plain);
    assert_eq!(arena.pending_destroyed(), 1);
    let charged = account.snapshot().pinned_bytes;
    let destroyed = arena.drain_destroyed();
    assert_eq!(&*destroyed, &[described_id]);
    assert_eq!(
        account.snapshot().pinned_bytes,
        charged,
        "a drained batch keeps its backing charged through cleanup"
    );
    drop(destroyed);
    assert!(
        account.snapshot().pinned_bytes < charged,
        "dropping the batch frees its backing charge"
    );
    assert_eq!(arena.pending_destroyed(), 0);
    assert!(
        !arena.track_sidecar(plain_id),
        "a destroyed slot cannot be marked"
    );
}

/// Ids are never reused, so a long session allocates ever higher chunk
/// numbers. With one early node pinning its chunk, the chunk table must still
/// hold entries for the live chunks only, not for every chunk the session
/// ever allocated and released.
#[test]
fn chunk_table_storage_follows_live_chunks_not_allocation_history() {
    let arena = NodeArena::default();
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
        inner.chunks.entries_in_use() <= 2 * live_chunks + CHUNK_WINDOW_SLACK,
        "{} table entries in use for {live_chunks} live chunks after {churned_chunks} released ones",
        inner.chunks.entries_in_use()
    );
    assert!(
        inner.chunks.held_entries() <= 4 * (2 * live_chunks + CHUNK_WINDOW_SLACK),
        "{} table entries held, headroom included, for {live_chunks} live chunks",
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

/// Chunk numbers only grow, so nearly every new chunk lands past the
/// window's end. The window must grow geometrically: adding chunks one past
/// the end reallocates it a logarithmic number of times, not once per chunk.
#[test]
fn chunk_window_grows_geometrically_as_chunks_are_added() {
    let mut table = ChunkTable::default();
    let chunks = 512u64;
    let mut reallocations = 0;
    let mut capacity = table.held_entries();
    for key in 0..chunks {
        table.get_or_insert_empty(key);
        if table.held_entries() != capacity {
            capacity = table.held_entries();
            reallocations += 1;
        }
    }
    assert_eq!(table.len(), chunks as usize);
    assert!(
        reallocations <= 8,
        "{reallocations} window reallocations while adding {chunks} chunks"
    );
    assert_eq!(
        table.keys().collect::<Vec<_>>(),
        (0..chunks).collect::<Vec<_>>()
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
    let arena = Arc::new(NodeArena::default());
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
