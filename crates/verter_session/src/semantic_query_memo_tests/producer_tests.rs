//! The resumable memo protocol: producers belong to tasks, a subscription is
//! a value its holder waits on when it chooses, and a producer settles
//! wherever its lease went.

use std::sync::Arc;

use super::producer::{Claim, Joined, ReadCapture, Recursion};
use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_type_engine::semantic_query::{PrimitiveKind, ResolveDeclKey, ScopeId};
use verter_type_expr::TopLevelOwnerId;

const KEYED: &str = "/producer/keyed.ts";

fn keyed_host() -> (VerterHost, [u8; 16]) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(KEYED.to_string()),
            input_id: KEYED.to_string(),
            source: Arc::from("export interface Target { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(KEYED)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert of the keyed file succeeds");
    let hash = host
        .ensure_indexed_ready(KEYED)
        .expect("keyed-file IndexedReady materialises")
        .whole_hash;
    (host, hash)
}

fn key() -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(KEYED),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: Arc::from("Target"),
    })
}

/// A complete build rooted on the keyed file at `hash`, so a subscriber on
/// the same view validates and reuses it.
fn keyed_output(
    store: &SemanticGraphStore,
    hash: [u8; 16],
) -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue> {
    let node = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let carrier = verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: KEYED.to_string(),
            hash,
        },
    ]));
    let mut output: verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<
        SemanticNodeId,
    > = (QueryResult::Value(node), empty_signature()).into();
    output.graph_carrier = Some(Box::new(carrier));
    output.self_root_canonicals = Arc::from([Arc::<str>::from(KEYED)]);
    output.into()
}

fn claim<'s>(
    store: &'s SemanticGraphStore,
    host: &VerterHost,
    task: &verter_execution::tasks::ExecutionTask,
) -> (Claim<'s>, super::producer::ClaimAttempt) {
    let mut capture = ReadCapture::default();
    let Ok(mut attempt) = store.begin_query_claim(host, key(), &mut capture) else {
        panic!("a cold key has no warm answer");
    };
    let claim = store.claim_query(host, &mut attempt, task, &mut capture);
    (claim, attempt)
}

/// A producer belongs to the task that claimed it: the same task claiming
/// the key again is same-path recursion, while a second task on the same
/// thread gets a subscription it can hold without blocking. The lease is a
/// value — it settles on another thread — and the held subscription then
/// receives the delivered result without running a build of its own.
#[test]
fn a_producer_belongs_to_its_task_and_a_subscription_waits_when_its_holder_chooses() {
    let (host, hash) = keyed_host();
    let store = SemanticGraphStore::new();
    let first = store.task_registry_for_tests().register_task();
    let second = store.task_registry_for_tests().register_task();

    let (claimed, _first_attempt) = claim(&store, &host, &first);
    let Claim::Produce(lease) = claimed else {
        panic!("the first claim of a cold key produces it");
    };
    let (again, _) = claim(&store, &host, &first);
    assert!(
        matches!(again, Claim::Recursive(Recursion::SamePath)),
        "the claiming task's own producer answers it with the same-path carrier"
    );
    let (subscribed, mut second_attempt) = claim(&store, &host, &second);
    let Claim::Subscribed(subscription) = subscribed else {
        panic!("another task's claim subscribes to the open producer");
    };

    let produced = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut capture = ReadCapture::default();
                let Ok(mut settled) = lease.settle(&host, keyed_output(&store, hash)) else {
                    panic!("an uncancelled producer settles");
                };
                settled
                    .admit(&host, &mut capture)
                    .expect("an uncancelled producer is admitted");
                settled.complete(&host, &mut capture)
            })
            .join()
            .expect("the producer thread completes")
    });
    let QueryResult::Value(value) = produced.value else {
        panic!("the producer completes with its value");
    };

    let mut capture = ReadCapture::default();
    match subscription.wait(&host, &mut second_attempt, &mut capture) {
        Joined::Read(read) => assert!(
            matches!(read.value, QueryResult::Value(joined) if joined == value),
            "the subscriber reads the producer's value"
        ),
        Joined::Retry => panic!("a same-view subscriber reuses the delivered result"),
        Joined::Recursive(recursion) => panic!("no cycle of waits exists: {recursion:?}"),
    }
    assert_eq!(store.stats_snapshot().joined_waits, 1);
    drop((first, second));
    assert_eq!(store.wait_graph_counts_for_tests(), (0, 0));
}

/// Same-path recursion is decided by the task's open producers, not by the
/// flight table: a producer whose flight an invalidation retired from the
/// table is still open, so its task's nested claim of the key still answers
/// with the carrier instead of building the key again inside itself.
#[test]
fn a_retired_flight_does_not_hide_its_open_producer_from_its_task() {
    let (host, _hash) = keyed_host();
    let store = SemanticGraphStore::new();
    let task = store.task_registry_for_tests().register_task();
    let (claimed, _attempt) = claim(&store, &host, &task);
    let Claim::Produce(lease) = claimed else {
        panic!("the first claim of a cold key produces it");
    };
    assert!(store.test_trigger_inflight_abort_impl(&key()));
    let (again, _) = claim(&store, &host, &task);
    assert!(
        matches!(again, Claim::Recursive(Recursion::SamePath)),
        "the open producer answers its own task's claim"
    );
    drop(lease);
    let (after, _) = claim(&store, &host, &task);
    assert!(
        matches!(after, Claim::Produce(_)),
        "once the producer closes, the task claims the key afresh"
    );
}

/// Two stores never share same-path identity: the same key open on one
/// store's task is no recursion for a claim on another store.
#[test]
fn same_path_identity_is_scoped_to_one_store() {
    let (host, _hash) = keyed_host();
    let first = SemanticGraphStore::new();
    let second = SemanticGraphStore::new();
    let first_execution = first.enter_execution();
    let (claimed, _attempt) = claim(&first, &host, first_execution.task());
    let Claim::Produce(_lease) = claimed else {
        panic!("the first claim of a cold key produces it");
    };
    assert!(first.is_same_path_claim(&key()));
    assert!(!second.is_same_path_claim(&key()));
    let second_execution = second.enter_execution();
    let (other, _) = claim(&second, &host, second_execution.task());
    assert!(matches!(other, Claim::Produce(_)));
}

/// An inline member flight belongs to the task that opened it for as long as
/// the flight is open — also after the synchronous entry that opened it has
/// returned, which is where an obligation root drains its members. A claim
/// on another thread therefore waits on the member instead of reading the
/// finished entry's retired task as a cycle and answering with the
/// recursion carrier, the ReturnOnly partial a consumer reports as an
/// unraisable source.
#[test]
fn a_member_flight_outliving_its_entry_is_waited_on_not_refused_as_a_cycle() {
    let (host, _hash) = keyed_host();
    let store = SemanticGraphStore::new();
    // The member flight opens inside a synchronous entry, which then returns
    // while the flight stays open.
    let flight = {
        let _entry = store.enter_execution();
        store
            .begin_inline_member_flight_for_tests(key())
            .expect("a cold key's member flight opens")
    };
    std::thread::scope(|scope| {
        let claimant = scope.spawn(|| {
            let mut execution = None;
            let mut capture = ReadCapture::default();
            match store.acquire_query(&host, key(), &mut execution, &mut capture) {
                super::producer::Acquired::Produce(_) => "produce",
                super::producer::Acquired::Read(_) => "read",
                super::producer::Acquired::Recursive(_) => "recursive",
            }
        });
        // The claimant parks on the open member flight ...
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while store.test_joiner_on_condvar_count() == 0 && !claimant.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "the claimant never parked"
            );
            std::thread::yield_now();
        }
        // ... and, once the member is abandoned, claims the key itself.
        store.abort_inline_member_flight(&flight);
        assert_eq!(
            claimant.join().expect("the claimant returns"),
            "produce",
            "a claim on an open member flight must wait for it, never answer a cycle"
        );
    });
    drop(flight);
    assert_eq!(store.wait_graph_counts_for_tests(), (0, 0));
}
