//! Deterministic task and wait-cycle tests.
//!
//! These tests cover the task/generation substrate directly and drive the
//! real semantic-query singleflight for the cross-thread nonpublication
//! guarantee.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use super::*;
use crate::{HostConfig, VerterHost};
use verter_type_engine::semantic_query::{ResolveDeclKey, ScopeId};
use verter_type_expr::TopLevelOwnerId;

fn ctx_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        owner: TopLevelOwnerId::ordinary_file(),
        local_scope: None,
        binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
            TopLevelOwnerId::ordinary_file(),
        ),
    }
}

fn key(name: &str) -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: scope("/wait-cycle.ts"),
        name: Arc::from(name),
    })
}

fn join_within<T: Send + 'static>(handle: thread::JoinHandle<T>, label: &str) -> T {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = tx.send(handle.join());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => panic!("{label} panicked"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label} deadlocked (join did not complete within 10s)")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{label} watchdog disconnected")
        }
    }
}

fn run_real_singleflight_cycle(task_count: usize) {
    let store = Arc::new(SemanticGraphStore::new());
    let rendezvous = Arc::new(Barrier::new(task_count));
    let keys: Arc<[SemanticQueryKey]> = (0..task_count)
        .map(|index| key(&format!("Task{index}")))
        .collect::<Vec<_>>()
        .into();
    let saw_return_only = Arc::new(AtomicBool::new(false));

    let handles = (0..task_count)
        .map(|index| {
            let store = Arc::clone(&store);
            let rendezvous = Arc::clone(&rendezvous);
            let keys = Arc::clone(&keys);
            let saw_return_only = Arc::clone(&saw_return_only);
            thread::spawn(move || {
                let host = ctx_host();
                let outer_key = keys[index].clone();
                let nested_key = keys[(index + 1) % keys.len()].clone();
                store.execute_cooperative(
                    &host,
                    outer_key,
                    || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    || {
                        rendezvous.wait();
                        let nested = store.execute_cooperative(
                            &host,
                            nested_key,
                            || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                            || -> (QueryResult<SemanticNodeId>, DepSignature) {
                                panic!("every nested key already has a cold owner")
                            },
                        );
                        saw_return_only.fetch_or(nested.cache_suppress, Ordering::SeqCst);
                        let mut output: verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<
                            _,
                        > = (nested.value, nested.dep_signature).into();
                        output.cache_suppress = nested.cache_suppress;
                        output.fold_partial(nested.result_is_partial);
                        output
                    },
                )
            })
        })
        .collect::<Vec<_>>();

    for (index, handle) in handles.into_iter().enumerate() {
        let read = join_within(handle, &format!("cycle task {index}"));
        assert!(
            matches!(read.value, QueryResult::Recursive(_)),
            "cycle task must receive the established recursion carrier, got {:?}",
            read.value
        );
        assert!(
            read.cache_suppress && read.result_is_partial,
            "cycle task must remain ReturnOnly + partial"
        );
    }
    assert!(
        saw_return_only.load(Ordering::SeqCst),
        "the edge that closed the cross-thread cycle must return ReturnOnly"
    );
    assert_eq!(
        store.memo_entry_count(),
        0,
        "cycle-derived values must never publish into the family memo"
    );
    assert_eq!(
        store.wait_graph_counts_for_tests(),
        (0, 0),
        "all execution tasks and wait edges must retire after the calls return"
    );
}

#[test]
fn real_singleflight_two_task_cycle_returns_return_only_and_never_publishes() {
    run_real_singleflight_cycle(2);
}

#[test]
fn real_singleflight_three_task_cycle_returns_return_only_and_never_publishes() {
    run_real_singleflight_cycle(3);
}
