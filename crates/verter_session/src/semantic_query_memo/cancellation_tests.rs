//! Cancellation discriminators for semantic-query singleflight.

use super::*;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::{PrimitiveKind, ResolveDeclKey, ScopeId};
use crate::{HostConfig, VerterHost};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use verter_type_expr::TopLevelOwnerId;

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn key(name: &'static str) -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/w/cancel.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: Arc::from(name),
    })
}

fn dep_signature(canonical: &'static str, hash: u8) -> DepSignature {
    Arc::from(
        vec![(
            Arc::<str>::from(canonical),
            crate::semantic_query::DepVersion::WholeHash([hash; 16]),
        )]
        .into_boxed_slice(),
    )
}

fn join_within<T: Send + 'static>(handle: std::thread::JoinHandle<T>, label: &str) -> T {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(_) => panic!("{label} did not finish"),
    }
}

#[test]
fn cancelled_joiner_detaches_without_aborting_live_winner() {
    let store = Arc::new(SemanticGraphStore::new());
    let query = key("DetachedJoiner");
    let builds = Arc::new(AtomicUsize::new(0));
    let winner_context = RequestContext::new(1, Arc::from("/w/cancel.ts"), false, None);
    let joiner_context = RequestContext::new(2, Arc::from("/w/cancel.ts"), false, None);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();

    let winner = {
        let store = Arc::clone(&store);
        let store_for_build = Arc::clone(&store);
        let query = query.clone();
        let builds = Arc::clone(&builds);
        let context = Arc::clone(&winner_context);
        std::thread::spawn(move || {
            let _guard = RequestContextGuard::install(context);
            let host = host();
            store.execute_cooperative(
                &host,
                query,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                move || {
                    builds.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    let node = store_for_build
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(node), empty_signature())
                },
            )
        })
    };
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("winner must enter cold build");

    let (joiner_done_tx, joiner_done_rx) = std::sync::mpsc::channel();
    let joiner = {
        let store = Arc::clone(&store);
        let query = query.clone();
        let context = Arc::clone(&joiner_context);
        std::thread::spawn(move || {
            let _guard = RequestContextGuard::install(context);
            let host = host();
            let result = store.execute_cooperative(
                &host,
                query,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || -> (QueryResult<SemanticNodeId>, DepSignature) {
                    panic!("a joiner must not execute while the winner is live")
                },
            );
            joiner_done_tx.send(result.clone()).unwrap();
            result
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while store.test_joiner_on_condvar_count() == 0 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(store.test_joiner_on_condvar_count() > 0);

    joiner_context.cancel();
    let detached = joiner_done_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("cancelled joiner must detach while winner remains blocked");
    assert!(matches!(
        detached.value,
        QueryResult::Error(QueryError::Cancelled)
    ));
    assert!(detached.cache_suppress && detached.result_is_partial);
    let _ = join_within(joiner, "cancelled joiner");

    release_tx.send(()).unwrap();
    let winner = join_within(winner, "live winner");
    assert!(matches!(winner.value, QueryResult::Value(_)));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(store.memo_entry_count(), 1);
}

#[test]
fn cancelled_leader_never_publishes_and_live_follower_retries_cold() {
    let store = Arc::new(SemanticGraphStore::new());
    let query = key("CancelledLeader");
    let builds = Arc::new(AtomicUsize::new(0));
    let leader_context = RequestContext::new(3, Arc::from("/w/cancel.ts"), false, None);
    let follower_context = RequestContext::new(4, Arc::from("/w/cancel.ts"), false, None);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();

    let leader = {
        let store = Arc::clone(&store);
        let store_for_build = Arc::clone(&store);
        let query = query.clone();
        let builds = Arc::clone(&builds);
        let context = Arc::clone(&leader_context);
        std::thread::spawn(move || {
            let _guard = RequestContextGuard::install(context);
            let host = host();
            store.execute_cooperative(
                &host,
                query,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                move || {
                    builds.fetch_add(1, Ordering::SeqCst);
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    let node = store_for_build
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(node), empty_signature())
                },
            )
        })
    };
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("leader must enter cold build");

    let follower = {
        let store = Arc::clone(&store);
        let store_for_build = Arc::clone(&store);
        let query = query.clone();
        let builds = Arc::clone(&builds);
        let context = Arc::clone(&follower_context);
        std::thread::spawn(move || {
            let _guard = RequestContextGuard::install(context);
            let host = host();
            store.execute_cooperative(
                &host,
                query,
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                move || {
                    builds.fetch_add(1, Ordering::SeqCst);
                    let node = store_for_build
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    (QueryResult::Value(node), empty_signature())
                },
            )
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while store.test_joiner_on_condvar_count() == 0 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(store.test_joiner_on_condvar_count() > 0);

    leader_context.cancel();
    release_tx.send(()).unwrap();

    let leader = join_within(leader, "cancelled leader");
    assert!(matches!(
        leader.value,
        QueryResult::Error(QueryError::Cancelled)
    ));
    assert!(leader.cache_suppress && leader.result_is_partial);

    let follower = join_within(follower, "retrying live follower");
    let follower_node = match follower.value {
        QueryResult::Value(node) => node,
        other => panic!("expected follower value, got {other:?}"),
    };
    assert!(matches!(
        *store.node_data(follower_node).unwrap(),
        SemanticNodeData::Primitive(PrimitiveKind::Number)
    ));
    assert_eq!(builds.load(Ordering::SeqCst), 2);
    assert_eq!(store.memo_entry_count(), 1);

    let warm_context = RequestContext::new(5, Arc::from("/w/cancel.ts"), false, None);
    let _guard = RequestContextGuard::install(warm_context);
    let host = host();
    let warm = store.execute_cooperative(
        &host,
        query,
        || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        || -> (QueryResult<SemanticNodeId>, DepSignature) {
            panic!("the live follower's result must be the sole warm value")
        },
    );
    assert!(matches!(warm.value, QueryResult::Value(node) if node == follower_node));
}

#[test]
fn post_admission_cancellation_preserves_aba_replacement_and_success_capture() {
    let store = Arc::new(SemanticGraphStore::new());
    let query = key("CancelledAdmissionWithReplacement");
    let context = RequestContext::new(6, Arc::from("/w/cancel.ts"), false, None);
    let host = host();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_cold_winner_post_admission_gate(Arc::clone(&barrier));

    let (worker, original_seq, replacement_seq, carrier, generation) =
        std::thread::scope(|scope| {
            let store_for_worker = Arc::clone(&store);
            let query_for_worker = query.clone();
            let context_for_worker = Arc::clone(&context);
            let worker = scope.spawn(move || {
                let _guard = RequestContextGuard::install(context_for_worker);
                let mut publication = None;
                let read = store_for_worker.execute_cooperative_value_capturing_publication(
                    &host,
                    query_for_worker,
                    || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        let node = store_for_worker
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                        (
                            QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                            empty_signature(),
                        )
                    },
                    &mut publication,
                );
                (read, publication)
            });

            barrier.wait();
            let carrier = store
                .entry_read_set_signature_for_tests(&query)
                .expect("the original candidate must be admitted");
            let generation = store
                .slot_candidate_generations_for_tests(&query)
                .into_iter()
                .next()
                .expect("the original candidate generation");
            let original_seq = store
                .slot_candidate_admission_seqs_for_tests(&query)
                .into_iter()
                .next()
                .expect("the original candidate admission token");
            let replacement = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            assert_eq!(
                store.publish_with_carrier_dispatch_and_generation_for_tests(
                    query.clone(),
                    QueryResult::Value(replacement),
                    carrier.clone(),
                    Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
                    empty_signature(),
                    generation,
                ),
                1,
                "the same-discriminant publish must replace the original admission"
            );
            let replacement_seq = store
                .slot_candidate_admission_seqs_for_tests(&query)
                .into_iter()
                .next()
                .expect("the replacement admission token");
            assert_ne!(replacement_seq, original_seq);
            context.cancel();
            barrier.wait();
            (
                worker.join().expect("original publisher"),
                original_seq,
                replacement_seq,
                carrier,
                generation,
            )
        });

    assert!(matches!(
        worker.0.value,
        QueryResult::Value(SemanticQueryValue::TypeNode(node))
            if matches!(
                store.node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::String))
            )
    ));
    let publication = worker
        .1
        .expect("successful admission retains its exact publication capture");
    assert_eq!(publication.admission_seq, original_seq);
    assert_eq!(publication.read_set_signature.facts, carrier.facts);
    assert_eq!(publication.validated_at_generation, generation);
    assert_eq!(
        store.slot_candidate_count_for_tests(&query),
        1,
        "post-admission cancellation must not remove the later replacement candidate"
    );
    let survivor = store
        .get_unvalidated(&query)
        .expect("the replacement candidate must survive");
    assert!(matches!(
        survivor.value,
        QueryResult::Value(node)
            if matches!(
                store.node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Number))
            )
    ));
    assert_eq!(
        store.slot_candidate_admission_seqs_for_tests(&query),
        vec![replacement_seq]
    );
    assert_eq!(store.memo_budget_tracked_len_for_test(), 1);
}

#[test]
fn post_admission_cancellation_keeps_same_discriminant_replacement_successful() {
    use crate::semantic_query::demand::MaterializedSet;

    let store = Arc::new(SemanticGraphStore::new());
    let query = key("SameDiscriminantAdmissionWins");
    let host = host();
    let generation = host.project_type_store().current_project_generation();
    let old = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    assert_eq!(
        store.publish_with_materialized_set_for_tests(
            query.clone(),
            QueryResult::Value(old),
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
            empty_signature(),
            generation,
            MaterializedSet::empty(),
        ),
        1
    );
    let old_seq = store.slot_candidate_admission_seqs_for_tests(&query)[0];
    let context = RequestContext::new(7, Arc::from("/w/cancel.ts"), false, None);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_cold_winner_post_admission_gate(Arc::clone(&barrier));

    let (read, publication, admitted_seq) = std::thread::scope(|scope| {
        let store_for_worker = Arc::clone(&store);
        let query_for_worker = query.clone();
        let context_for_worker = Arc::clone(&context);
        let worker = scope.spawn(move || {
            let _guard = RequestContextGuard::install(context_for_worker);
            let mut publication = None;
            let read = store_for_worker.execute_cooperative_value_capturing_publication(
                &host,
                query_for_worker,
                || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    let node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (
                        QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                        empty_signature(),
                    )
                },
                &mut publication,
            );
            (read, publication)
        });
        barrier.wait();
        let admitted_seq = store.slot_candidate_admission_seqs_for_tests(&query)[0];
        assert_ne!(admitted_seq, old_seq);
        assert_eq!(store.slot_candidate_count_for_tests(&query), 1);
        context.cancel();
        barrier.wait();
        let (read, publication) = worker.join().expect("same-discriminant publisher");
        (read, publication, admitted_seq)
    });

    assert!(matches!(
        read.value,
        QueryResult::Value(SemanticQueryValue::TypeNode(node))
            if matches!(
                store.node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::String))
            )
    ));
    assert_eq!(
        publication.expect("admission-wins capture").admission_seq,
        admitted_seq
    );
    assert_eq!(
        store.slot_candidate_admission_seqs_for_tests(&query),
        vec![admitted_seq]
    );
    assert_eq!(store.memo_budget_tracked_len_for_test(), 1);
}

#[test]
fn post_admission_cancellation_keeps_cap_lru_eviction_committed() {
    use crate::semantic_query::demand::MaterializedSet;

    let store = Arc::new(SemanticGraphStore::new());
    let query = key("CapLruAdmissionWins");
    for generation in 100..104 {
        let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        assert_eq!(
            store.publish_with_materialized_set_for_tests(
                query.clone(),
                QueryResult::Value(node),
                crate::fact_signature_helpers::ReadSetSignature::empty(),
                Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
                empty_signature(),
                generation,
                MaterializedSet::empty(),
            ),
            1
        );
    }
    assert_eq!(
        store.slot_candidate_generations_for_tests(&query),
        vec![100, 101, 102, 103]
    );
    let host = host();
    let admitted_generation = host.project_type_store().current_project_generation();
    let context = RequestContext::new(8, Arc::from("/w/cancel.ts"), false, None);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_cold_winner_post_admission_gate(Arc::clone(&barrier));

    let (read, publication) = std::thread::scope(|scope| {
        let store_for_worker = Arc::clone(&store);
        let query_for_worker = query.clone();
        let context_for_worker = Arc::clone(&context);
        let worker = scope.spawn(move || {
            let _guard = RequestContextGuard::install(context_for_worker);
            let mut publication = None;
            let read = store_for_worker.execute_cooperative_value_capturing_publication(
                &host,
                query_for_worker,
                || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    let node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (
                        QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                        empty_signature(),
                    )
                },
                &mut publication,
            );
            (read, publication)
        });
        barrier.wait();
        assert_eq!(
            store.slot_candidate_generations_for_tests(&query),
            vec![101, 102, 103, admitted_generation],
            "successful admission evicts the LRU-front candidate"
        );
        context.cancel();
        barrier.wait();
        worker.join().expect("cap/LRU publisher")
    });

    assert!(matches!(read.value, QueryResult::Value(_)));
    assert_eq!(
        publication
            .expect("cap/LRU admission capture")
            .validated_at_generation,
        admitted_generation
    );
    assert_eq!(
        store.slot_candidate_generations_for_tests(&query),
        vec![101, 102, 103, admitted_generation]
    );
    assert_eq!(store.memo_budget_tracked_len_for_test(), 1);
}

#[test]
fn post_admission_cancellation_keeps_global_fifo_eviction_committed() {
    let store = Arc::new(SemanticGraphStore::new_with_memo_budget_for_test(2));
    let first = key("GlobalFifoFirst");
    let second = key("GlobalFifoSecond");
    let admitted = key("GlobalFifoAdmissionWins");
    let seed = |key: SemanticQueryKey, canonical, hash| {
        let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        assert_eq!(
            store.publish_with_carrier_and_dispatch_for_tests(
                key,
                QueryResult::Value(node),
                crate::fact_signature_helpers::ReadSetSignature::empty(),
                Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
                dep_signature(canonical, hash),
            ),
            1
        );
    };
    seed(first.clone(), "/w/fifo-first.ts", 1);
    seed(second.clone(), "/w/fifo-second.ts", 2);
    let host = host();
    let context = RequestContext::new(9, Arc::from("/w/cancel.ts"), false, None);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_cold_winner_post_admission_gate(Arc::clone(&barrier));

    let read = std::thread::scope(|scope| {
        let store_for_worker = Arc::clone(&store);
        let admitted_for_worker = admitted.clone();
        let context_for_worker = Arc::clone(&context);
        let worker = scope.spawn(move || {
            let _guard = RequestContextGuard::install(context_for_worker);
            store_for_worker.execute_cooperative(
                &host,
                admitted_for_worker,
                || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    let node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (QueryResult::Value(node), dep_signature("/w/fifo-new.ts", 3))
                },
            )
        });
        barrier.wait();
        assert!(store.get_unvalidated(&first).is_none());
        assert!(store.get_unvalidated(&second).is_some());
        assert!(store.get_unvalidated(&admitted).is_some());
        assert_eq!(store.memo_budget_tracked_len_for_test(), 2);
        assert_eq!(store.canonical_to_entries_count("/w/fifo-first.ts"), 0);
        assert_eq!(store.canonical_to_entries_count("/w/fifo-second.ts"), 1);
        assert_eq!(store.canonical_to_entries_count("/w/fifo-new.ts"), 1);
        context.cancel();
        barrier.wait();
        worker.join().expect("global FIFO publisher")
    });

    assert!(matches!(read.value, QueryResult::Value(_)));
    assert!(store.get_unvalidated(&first).is_none());
    assert!(store.get_unvalidated(&second).is_some());
    assert!(store.get_unvalidated(&admitted).is_some());
    assert_eq!(store.memo_budget_tracked_len_for_test(), 2);
    assert_eq!(store.canonical_to_entries_count("/w/fifo-first.ts"), 0);
    assert_eq!(store.canonical_to_entries_count("/w/fifo-second.ts"), 1);
    assert_eq!(store.canonical_to_entries_count("/w/fifo-new.ts"), 1);
}

#[test]
fn post_admission_cancellation_keeps_parent_and_prefix_backfill() {
    use crate::project_semantic_dispatch::walk::{PrefixBackfill, QueryBuildOutput};
    use crate::semantic_query::demand::MaterializedSet;

    let store = Arc::new(SemanticGraphStore::new());
    let parent = key("PrefixParentAdmissionWins");
    let child = key("PrefixChildAdmissionWins");
    let host = host();
    let context = RequestContext::new(10, Arc::from("/w/cancel.ts"), false, None);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_cold_winner_post_admission_gate(Arc::clone(&barrier));

    let read = std::thread::scope(|scope| {
        let store_for_worker = Arc::clone(&store);
        let parent_for_worker = parent.clone();
        let child_for_worker = child.clone();
        let context_for_worker = Arc::clone(&context);
        let worker = scope.spawn(move || {
            let _guard = RequestContextGuard::install(context_for_worker);
            store_for_worker.execute_cooperative(
                &host,
                parent_for_worker.clone(),
                || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    let parent_node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                    let child_node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    QueryBuildOutput {
                        result: QueryResult::Value(parent_node),
                        dep_signature: dep_signature("/w/prefix.ts", 4),
                        walker_diagnostics: Vec::new(),
                        cache_suppress: false,
                        result_is_partial: false,
                        taint: crate::semantic_query::ResultTaint::Clean,
                        observed_self_roots: Vec::new(),
                        graph_carrier: None,
                        self_root_canonicals: Arc::from([]),
                        pending_prefix_backfills: vec![PrefixBackfill {
                            satisfied_projection: MaterializedSet::single(
                                super::family::requested_point_for_key(&child_for_worker),
                            ),
                            key: child_for_worker,
                            node: child_node,
                        }],
                        satisfied_projection: MaterializedSet::single(
                            super::family::requested_point_for_key(&parent_for_worker),
                        ),
                    }
                },
            )
        });
        barrier.wait();
        assert!(store.get_unvalidated(&parent).is_some());
        assert!(store.get_unvalidated(&child).is_none());
        context.cancel();
        barrier.wait();
        worker.join().expect("prefix publisher")
    });

    assert!(matches!(read.value, QueryResult::Value(_)));
    assert!(store.get_unvalidated(&parent).is_some());
    assert!(store.get_unvalidated(&child).is_some());
    assert_eq!(store.memo_budget_tracked_len_for_test(), 2);
    assert_eq!(store.canonical_to_entries_count("/w/prefix.ts"), 2);
}

#[test]
fn cancellation_before_admission_remains_cancelled_and_unpublished() {
    let store = Arc::new(SemanticGraphStore::new());
    let query = key("PreAdmissionCancellation");
    let host = host();
    let context = RequestContext::new(11, Arc::from("/w/cancel.ts"), false, None);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();

    let (read, publication) = std::thread::scope(|scope| {
        let store_for_worker = Arc::clone(&store);
        let query_for_worker = query.clone();
        let context_for_worker = Arc::clone(&context);
        let worker = scope.spawn(move || {
            let _guard = RequestContextGuard::install(context_for_worker);
            let mut publication = None;
            let read = store_for_worker.execute_cooperative_value_capturing_publication(
                &host,
                query_for_worker,
                || store_for_worker.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    let node = store_for_worker
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                    (
                        QueryResult::Value(SemanticQueryValue::TypeNode(node)),
                        dep_signature("/w/pre-admission.ts", 5),
                    )
                },
                &mut publication,
            );
            (read, publication)
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("cold build must reach the pre-admission window");
        context.cancel();
        release_tx.send(()).unwrap();
        worker.join().expect("pre-admission publisher")
    });

    assert!(matches!(
        read.value,
        QueryResult::Error(QueryError::Cancelled)
    ));
    assert!(publication.is_none());
    assert_eq!(store.slot_candidate_count_for_tests(&query), 0);
    assert_eq!(store.memo_budget_tracked_len_for_test(), 0);
    assert_eq!(store.canonical_to_entries_count("/w/pre-admission.ts"), 0);
}
