//! Deterministic cooperative wait-cycle tests.
//!
//! These tests cover the owner/generation substrate directly and drive the
//! real semantic-query singleflight for the cross-thread nonpublication
//! guarantee.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use super::wait_cycle::{WaitCycle, WaitForGraph};
use super::*;
use crate::semantic_query::{ResolveDeclKey, ScopeId};
use crate::{HostConfig, VerterHost};
use verter_type_expr::TopLevelOwnerId;

fn ctx_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn scope(canonical: &str) -> ScopeId {
    ScopeId {
        canonical_id: Arc::from(canonical),
        owner: TopLevelOwnerId::ordinary_file(),
        local_scope: None,
        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
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

#[test]
fn two_owner_wait_cycle_is_detected_before_parking() {
    let graph = WaitForGraph::default();
    let owner_a = graph.register_owner();
    let owner_b = graph.register_owner();

    let _a_waits_for_b = graph
        .register_wait(owner_a.owner(), owner_b.owner())
        .expect("first edge is acyclic");
    assert!(matches!(
        graph.register_wait(owner_b.owner(), owner_a.owner()),
        Err(WaitCycle)
    ));
    assert_eq!(graph.wait_count_for_tests(), 1);
}

#[test]
fn three_owner_wait_cycle_is_detected_before_parking() {
    let graph = WaitForGraph::default();
    let owner_a = graph.register_owner();
    let owner_b = graph.register_owner();
    let owner_c = graph.register_owner();

    let _a_waits_for_b = graph
        .register_wait(owner_a.owner(), owner_b.owner())
        .expect("first edge is acyclic");
    let _b_waits_for_c = graph
        .register_wait(owner_b.owner(), owner_c.owner())
        .expect("second edge is acyclic");
    assert!(matches!(
        graph.register_wait(owner_c.owner(), owner_a.owner()),
        Err(WaitCycle)
    ));
    assert_eq!(graph.wait_count_for_tests(), 2);
}

#[test]
fn acyclic_wait_edges_register_and_clean_up_independently() {
    let graph = WaitForGraph::default();
    let owner_a = graph.register_owner();
    let owner_b = graph.register_owner();
    let owner_c = graph.register_owner();

    let a_waits_for_b = graph
        .register_wait(owner_a.owner(), owner_b.owner())
        .expect("a -> b is acyclic");
    let b_waits_for_c = graph
        .register_wait(owner_b.owner(), owner_c.owner())
        .expect("b -> c is acyclic");
    assert_eq!(graph.wait_count_for_tests(), 2);

    drop(a_waits_for_b);
    assert_eq!(graph.wait_count_for_tests(), 1);
    drop(b_waits_for_c);
    assert_eq!(graph.wait_count_for_tests(), 0);
}

#[test]
fn stale_generation_cleanup_cannot_remove_reused_owner_or_wait() {
    let graph = WaitForGraph::default();
    let first = graph.register_owner();
    let stale = first.owner();
    drop(first);

    let reused = graph.register_owner();
    let target = graph.register_owner();
    assert_eq!(reused.owner().id(), stale.id(), "owner slot must be reused");
    assert_ne!(
        reused.owner().generation(),
        stale.generation(),
        "slot reuse must advance the generation"
    );
    let _wait = graph
        .register_wait(reused.owner(), target.owner())
        .expect("reused owner wait is acyclic");

    graph.unregister_owner_for_tests(stale);
    graph.remove_wait_for_tests(stale, target.owner());
    assert!(graph.is_active_for_tests(reused.owner()));
    assert_eq!(graph.wait_count_for_tests(), 1);
}

#[test]
fn panic_and_cancel_style_early_return_clean_wait_registrations() {
    let graph = WaitForGraph::default();
    let target = graph.register_owner();

    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let waiter = graph.register_owner();
        let _wait = graph
            .register_wait(waiter.owner(), target.owner())
            .expect("panic-path wait is acyclic");
        panic!("simulated owner panic");
    }));
    assert!(panicked.is_err());
    assert_eq!(graph.wait_count_for_tests(), 0);
    assert_eq!(graph.active_owner_count_for_tests(), 1);

    fn cancelled_early(graph: &WaitForGraph, target: super::wait_cycle::ExecutionOwner) {
        let waiter = graph.register_owner();
        let _wait = graph
            .register_wait(waiter.owner(), target)
            .expect("cancel-path wait is acyclic");
        // A cancellation return drops both RAII registrations.
    }
    cancelled_early(&graph, target.owner());
    assert_eq!(graph.wait_count_for_tests(), 0);
    assert_eq!(graph.active_owner_count_for_tests(), 1);
}

fn run_real_singleflight_cycle(owner_count: usize) {
    let store = Arc::new(SemanticGraphStore::new());
    let rendezvous = Arc::new(Barrier::new(owner_count));
    let keys: Arc<[SemanticQueryKey]> = (0..owner_count)
        .map(|index| key(&format!("Owner{index}")))
        .collect::<Vec<_>>()
        .into();
    let saw_return_only = Arc::new(AtomicBool::new(false));

    let handles = (0..owner_count)
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
                        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                            _,
                        > = (nested.value, nested.dep_signature).into();
                        output.cache_suppress = nested.cache_suppress;
                        output.result_is_partial = nested.result_is_partial;
                        output
                    },
                )
            })
        })
        .collect::<Vec<_>>();

    for (index, handle) in handles.into_iter().enumerate() {
        let read = join_within(handle, &format!("cycle owner {index}"));
        assert!(
            matches!(read.value, QueryResult::Recursive(_)),
            "cycle owner must receive the established recursion carrier, got {:?}",
            read.value
        );
        assert!(
            read.cache_suppress && read.result_is_partial,
            "cycle owner must remain ReturnOnly + partial"
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
        "all execution owners and wait edges must retire after the calls return"
    );
}

#[test]
fn real_singleflight_two_owner_cycle_returns_return_only_and_never_publishes() {
    run_real_singleflight_cycle(2);
}

#[test]
fn real_singleflight_three_owner_cycle_returns_return_only_and_never_publishes() {
    run_real_singleflight_cycle(3);
}
