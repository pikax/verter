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
        },
        name: Arc::from(name),
    })
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
