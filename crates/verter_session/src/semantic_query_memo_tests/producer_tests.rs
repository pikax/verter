//! The resumable memo protocol: producers belong to tasks, a subscription is
//! a value its holder waits on when it chooses, and a producer settles
//! wherever its lease went.

use std::sync::Arc;

use super::producer::{Claim, Joined, ReadCapture, Recursion};
use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_type_engine::resolver_core::fact_validation_port::FactValidation;
use verter_type_engine::semantic_query::{PrimitiveKind, ResolveDeclKey, ScopeId};
use verter_type_expr::TopLevelOwnerId;

const KEYED: &str = "/producer/keyed.ts";

#[test]
fn counted_zero_retention_admission_returns_an_owned_complete_delivery() {
    use verter_session_query::retention::{
        RetentionLimits, SemanticRetentionAccount, StoreAccount,
    };
    use verter_type_engine::semantic_query::LiteralValue;
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(
        SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: 0,
            ..RetentionLimits::defaults()
        }),
    ));
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                (
                    QueryResult::Value(store.intern_node(SemanticNodeData::Literal(
                        LiteralValue::String("complete but uncached".into()),
                    ))),
                    empty_signature(),
                )
            },
        )
    });
    assert!(matches!(read.value().clone(), QueryResult::Value(_)));
    assert!(
        !read.result_is_partial(),
        "retention pressure does not change computation completeness"
    );
    harness.run(|store| {
        assert_eq!(store.memo_entry_count(), 0);
        assert_eq!(store.retention_account().snapshot().retained_bytes, 0);
    });
    assert_eq!(harness.live_nodes(), 1);
    drop(read);
    assert_eq!(harness.live_nodes(), 0);
}

#[test]
fn counted_sealed_refusal_owns_nonempty_key_value_and_diagnostic_operands() {
    use verter_type_engine::project_semantic_dispatch::cost_receipt::{
        BudgetProfile, BudgetProfileSpec, COST_MODEL_REVISION,
    };
    use verter_type_engine::project_semantic_dispatch::walk::{
        QueryBuildOutput, ShallowDiagnostic,
    };
    use verter_type_engine::semantic_query::{LiteralValue, PartialReasonSet};
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    harness.run(|store| {
        let key_node = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "refused operand".into(),
        )));
        let read = store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                let result = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "partial result".into(),
                )));
                let diagnostic = store.intern_node(SemanticNodeData::Literal(
                    LiteralValue::String("refusal diagnostic".into()),
                ));
                let mut output: QueryBuildOutput<SemanticNodeId> =
                    (QueryResult::Value(result), empty_signature()).into();
                output.cache_suppress = true;
                output.mark_partial_with(PartialReasonSet::PROJECTION_WORK_LIMIT);
                // This fixture completed the literal source before refusing reuse.
                output = output.with_positional_guarantee_for_tests();
                output
                    .walker_diagnostics
                    .push(ShallowDiagnostic::OpenConditional { node: diagnostic });
                output
            },
        );
        let read = read.map_value(|result| match result {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(node)),
            _ => panic!("partial value still returns its complete carrier"),
        });
        let profile = BudgetProfile::intern(BudgetProfileSpec {
            work: 1,
            bytes: 1024,
            query_depth: 2,
            instantiation_depth: 2,
            tail_steps: 2,
            request_operations: 0,
            relation_comparisons: 2,
            cost_model_revision: COST_MODEL_REVISION,
        });
        assert!(store.seal_refusal_for_tests(
            &host,
            SemanticQueryKey::ClassifyTruthinessDomain { subject: key_node },
            profile,
            store.refusal_entry_for_tests(&host),
            read,
            verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from([]))
        ));
    });
    assert_eq!(
        harness.live_nodes(),
        3,
        "the summary owns its key and actual partial delivery"
    );
    harness.run(|store| {
        assert_eq!(store.refusal_summary_count_for_tests(), 1);
        store.invalidate_all();
    });
    assert_eq!(harness.live_nodes(), 0);
}

#[test]
fn counted_outer_build_keeps_a_consumed_warm_read_through_cache_eviction() {
    use verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput;
    use verter_type_engine::semantic_query::LiteralValue;
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    harness.run(|store| {
        drop(store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                (
                    QueryResult::Value(store.intern_node(SemanticNodeData::Literal(
                        LiteralValue::String("warm dependency".into()),
                    ))),
                    empty_signature(),
                )
            },
        ));
    });
    let mut outer_key = key();
    if let SemanticQueryKey::ResolveDecl(ref mut key) = outer_key {
        key.name = verter_type_expr::OwnedStr::from("Outer");
    }
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            outer_key,
            || panic!("no recursion"),
            || {
                let inner = store.execute_cooperative(
                    &host,
                    key(),
                    || panic!("no recursion"),
                    || -> (QueryResult<SemanticNodeId>, DepSignature) {
                        panic!("must read warm");
                    },
                );
                let QueryResult::Value(node) = inner.value().clone() else {
                    panic!("complete dependency");
                };
                store.invalidate_all();
                drop(inner);
                assert!(
                    store.node_is_live(node),
                    "outer construction owns the consumed dependency"
                );
                let mut output: QueryBuildOutput<SemanticNodeId> =
                    (QueryResult::Value(node), empty_signature()).into();
                output.cache_suppress = true;
                output
            },
        )
    });
    assert_eq!(harness.live_nodes(), 1);
    drop(read);
    assert_eq!(harness.live_nodes(), 0);
}

#[test]
fn counted_cancelled_producer_releases_unaudited_intermediates() {
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::semantic_query::LiteralValue;
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    let context = RequestContext::new(1, Arc::from(KEYED), false, None);
    let _request = RequestContextGuard::install(Arc::clone(&context));
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                let node = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "cancelled".into(),
                )));
                context.cancel();
                (QueryResult::Value(node), empty_signature())
            },
        )
    });
    assert!(matches!(
        read.value().clone(),
        QueryResult::Error(QueryError::Cancelled)
    ));
    assert_eq!(harness.live_nodes(), 0);
    harness.run(|store| assert_eq!(store.memo_entry_count(), 0));
}

#[test]
fn counted_return_only_delivery_owns_value_and_actual_diagnostic_after_store_drop() {
    use verter_type_engine::project_semantic_dispatch::walk::{
        QueryBuildOutput, ShallowDiagnostic,
    };
    use verter_type_engine::semantic_query::LiteralValue;
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                let result = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "result".into(),
                )));
                let diagnostic = store.intern_node(SemanticNodeData::Literal(
                    LiteralValue::String("diagnostic".into()),
                ));
                let mut output: QueryBuildOutput<SemanticNodeId> =
                    (QueryResult::Value(result), empty_signature()).into();
                output.cache_suppress = true;
                output
                    .walker_diagnostics
                    .push(ShallowDiagnostic::OpenConditional { node: diagnostic });
                output
            },
        )
    });
    assert!(read.cache_suppress());
    assert_eq!(read.walker_diagnostics().len(), 1);
    assert_eq!(harness.live_nodes(), 2);
    harness.run(|store| assert_eq!(store.memo_entry_count(), 0));
    drop(harness);
    assert_eq!(
        read.lease().live_nodes_for_tests(),
        2,
        "delivery itself owns both nodes"
    );
    let cloned = read.clone();
    drop(read);
    assert_eq!(cloned.lease().live_nodes_for_tests(), 2);
}

#[test]
fn counted_recursion_delivery_owns_its_returned_sentinel() {
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, _) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    let read = harness.run(|store| {
        store.with_same_path_inflight_for_test(key(), || {
            store.execute_cooperative(
                &host,
                key(),
                || store.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                || -> (QueryResult<SemanticNodeId>, DepSignature) {
                    panic!("recursive producer must not run")
                },
            )
        })
    });
    assert!(matches!(read.value().clone(), QueryResult::Recursive(_)));
    assert_eq!(harness.live_nodes(), 1);
    drop(harness);
    assert_eq!(read.lease().live_nodes_for_tests(), 1);
}

#[test]
fn counted_cache_and_reader_can_drop_in_either_order() {
    use verter_type_engine::semantic_query::LiteralValue;
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    for cache_first in [true, false] {
        let (host, _) = keyed_host();
        let harness = CountedOwnershipHarness::new();
        let read = harness.run(|store| {
            store.execute_cooperative(
                &host,
                key(),
                || panic!("no recursion"),
                || {
                    (
                        QueryResult::Value(store.intern_node(SemanticNodeData::Literal(
                            LiteralValue::String("cached".into()),
                        ))),
                        empty_signature(),
                    )
                },
            )
        });
        harness.run(|store| assert_eq!(store.memo_entry_count(), 1));
        if cache_first {
            harness.run(|store| {
                store.invalidate_all();
            });
            assert_eq!(harness.live_nodes(), 1);
            drop(read);
        } else {
            drop(read);
            assert_eq!(harness.live_nodes(), 1);
            harness.run(|store| {
                store.invalidate_all();
            });
        }
        assert_eq!(harness.live_nodes(), 0);
    }
}

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
            canonical_id: verter_type_expr::OwnedStr::from(KEYED),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_type_expr::OwnedStr::from("Target"),
    })
}

/// A complete build rooted on the keyed file at `hash`, so a subscriber on
/// the same view validates and reuses it.
fn keyed_output(
    store: &SemanticGraphStore,
    hash: [u8; 16],
) -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue> {
    keyed_output_of(
        store,
        hash,
        SemanticNodeData::Primitive(PrimitiveKind::String),
    )
}

/// [`keyed_output`] delivering `data`.
fn keyed_output_of(
    store: &SemanticGraphStore,
    hash: [u8; 16],
    data: SemanticNodeData,
) -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue> {
    let node = store.intern_node(data);
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
    let flags = host.request_flags();
    let Ok(mut attempt) = store.begin_query_claim(host, flags, key(), &mut capture) else {
        panic!("a cold key has no warm answer");
    };
    let claim = store.claim_query(host, flags, &mut attempt, task, &mut capture);
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

    let receipt = lease.fixture_receipt();
    let produced = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut capture = ReadCapture::default();
                let flags = host.request_flags();
                let Ok(mut settled) = lease.settle(
                    &host,
                    flags,
                    keyed_output(&store, hash),
                    Some(Arc::clone(&receipt)),
                ) else {
                    panic!("an uncancelled producer settles");
                };
                settled
                    .admit(&host, flags, &mut capture)
                    .expect("an uncancelled producer is admitted");
                settled.complete(&host, &mut capture)
            })
            .join()
            .expect("the producer thread completes")
    });
    let QueryResult::Value(value) = produced.value().clone() else {
        panic!("the producer completes with its value");
    };
    assert_eq!(
        produced.fact_status().clone(),
        verter_type_engine::semantic_query::FactStatus::Complete
    );

    let mut capture = ReadCapture::default();
    match subscription.wait(
        &host,
        host.request_flags(),
        &mut second_attempt,
        &mut capture,
    ) {
        Joined::Read(read) => {
            assert_eq!(read.fact_status(), produced.fact_status());
            assert!(
                matches!(read.value().clone(), QueryResult::Value(joined) if joined == value),
                "the subscriber reads the producer's value"
            );
            assert!(
                read.receipt()
                    .priced()
                    .is_some_and(|delivered| Arc::ptr_eq(delivered, &receipt)),
                "the subscriber's read carries the producer's receipt, to replay it as a warm read would"
            );
        }
        Joined::Retry => panic!("a same-view subscriber reuses the delivered result"),
        Joined::Recursive(recursion) => panic!("no cycle of waits exists: {recursion:?}"),
    }
    assert_eq!(store.stats_snapshot().joined_waits, 1);
    drop((first, second));
    assert_eq!(store.wait_graph_counts_for_tests(), (0, 0));
}

/// A subscriber joined to another task's flight receives a read that owns
/// the delivered value in its own right: after the producer's read, the
/// cache and the construction scope are gone, the joined read still reads
/// the value, and its drop releases it.
#[test]
fn a_joined_read_owns_the_value_after_the_producer_and_the_cache_are_gone() {
    use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
    let (host, hash) = keyed_host();
    let harness = CountedOwnershipHarness::new();
    let (produced, joined) = harness.run(|store| {
        let owner = store.task_registry_for_tests().register_task();
        let follower = store.task_registry_for_tests().register_task();
        let (claimed, _) = claim(store, &host, &owner);
        let Claim::Produce(lease) = claimed else {
            panic!("the first claim of a cold key produces it");
        };
        let (subscribed, mut attempt) = claim(store, &host, &follower);
        let Claim::Subscribed(subscription) = subscribed else {
            panic!("another task's claim subscribes to the open producer");
        };
        let receipt = lease.fixture_receipt();
        let mut capture = ReadCapture::default();
        let flags = host.request_flags();
        let output = keyed_output_of(
            store,
            hash,
            SemanticNodeData::Literal(verter_type_expr::LiteralValue::String("joined".into())),
        );
        let Ok(mut settled) = lease.settle(&host, flags, output, Some(receipt)) else {
            panic!("an uncancelled producer settles");
        };
        settled
            .admit(&host, flags, &mut capture)
            .expect("an uncancelled producer is admitted");
        let produced = settled.complete(&host, &mut capture);
        let Joined::Read(joined) = subscription.wait(&host, flags, &mut attempt, &mut capture)
        else {
            panic!("a same-view subscriber reads the delivered result");
        };
        (produced, joined)
    });
    harness.run(|store| store.invalidate_all());
    drop(produced);
    let QueryResult::Value(SemanticQueryValue::TypeNode(node)) = joined.value().clone() else {
        panic!("the subscriber receives the producer's node: {joined:?}");
    };
    assert_eq!(harness.live_nodes(), 1, "the joined read owns the value");
    let payload = joined.node(node).unwrap().read().unwrap();
    assert!(matches!(
        &*payload,
        SemanticNodeData::Literal(verter_type_expr::LiteralValue::String(text)) if text == "joined"
    ));
    drop((payload, joined));
    assert_eq!(
        harness.live_nodes(),
        0,
        "the joined read's drop releases it"
    );
}

#[test]
fn an_unavailable_producer_broadcasts_its_fact_without_a_candidate_value() {
    use verter_type_engine::semantic_query::{FactStatus, PartialReasonSet, QueryError};
    let (host, hash) = keyed_host();
    let store = SemanticGraphStore::new();
    let owner = store.task_registry_for_tests().register_task();
    let follower = store.task_registry_for_tests().register_task();
    let (claimed, _) = claim(&store, &host, &owner);
    let Claim::Produce(lease) = claimed else {
        panic!("cold owner must produce")
    };
    let (subscribed, mut attempt) = claim(&store, &host, &follower);
    let Claim::Subscribed(subscription) = subscribed else {
        panic!("follower must subscribe")
    };
    let mut output = keyed_output(&store, hash);
    output.add_partial_reasons(PartialReasonSet::MISSING_DEPENDENCY);
    output.cache_suppress = true;
    let receipt = lease.fixture_receipt();
    let mut settled = lease
        .settle(&host, host.request_flags(), output, Some(receipt))
        .expect("valid unfinished execution settles");
    let mut capture = ReadCapture::default();
    settled
        .admit(&host, host.request_flags(), &mut capture)
        .unwrap();
    let produced = settled.complete(&host, &mut capture);
    let Joined::Read(joined) =
        subscription.wait(&host, host.request_flags(), &mut attempt, &mut capture)
    else {
        panic!("same-view priced follower must receive the fact")
    };
    assert_eq!(produced.fact_status(), joined.fact_status());
    for read in [produced, joined] {
        assert!(matches!(read.fact_status(), FactStatus::Unavailable(_)));
        assert!(matches!(
            read.value().clone(),
            QueryResult::Error(QueryError::IncompleteSemanticOperand { .. })
        ));
        assert!(read.result_is_partial() && read.cache_suppress());
        assert!(read
            .partial_reasons()
            .contains(PartialReasonSet::MISSING_DEPENDENCY));
    }
    assert_eq!(store.slot_candidate_count_for_tests(&key()), 0);
}

/// A producer whose demand ran out of its allowance completes its flight
/// with an incomplete result and no receipt: where it stopped is that
/// demand's, not the key's. A subscriber joined to the flight — on the
/// same view, so the view gate alone would reuse it — never takes it: it
/// claims again and produces the key itself, on a flight of its own, so
/// it stops wherever its own evaluation stops and pays what it computes.
#[test]
fn a_subscriber_never_takes_another_demands_unpriced_partial() {
    let (host, hash) = keyed_host();
    let store = SemanticGraphStore::new();
    let producer = store.task_registry_for_tests().register_task();
    let follower = store.task_registry_for_tests().register_task();

    let (claimed, _producer_attempt) = claim(&store, &host, &producer);
    let Claim::Produce(lease) = claimed else {
        panic!("the first claim of a cold key produces it");
    };
    let (subscribed, mut follower_attempt) = claim(&store, &host, &follower);
    let Claim::Subscribed(subscription) = subscribed else {
        panic!("another task's claim subscribes to the open producer");
    };

    // The producer's demand tripped inside the build: a partial, with no
    // receipt sealed for it.
    let mut partial = keyed_output(&store, hash);
    partial.mark_partial_with(
        verter_type_engine::semantic_query::PartialReasonSet::PROJECTION_WORK_LIMIT,
    );
    partial.cache_suppress = true;
    {
        let mut capture = ReadCapture::default();
        let flags = host.request_flags();
        let Ok(mut settled) = lease.settle(&host, flags, partial, None) else {
            panic!("an uncancelled producer settles");
        };
        settled
            .admit(&host, flags, &mut capture)
            .expect("an uncancelled producer completes");
        let read = settled.complete(&host, &mut capture);
        assert!(read.result_is_partial() && read.receipt().priced().is_none());
    }

    let mut capture = ReadCapture::default();
    assert!(
        matches!(
            subscription.wait(
                &host,
                host.request_flags(),
                &mut follower_attempt,
                &mut capture,
            ),
            Joined::Retry
        ),
        "the follower does not inherit the producer's resource stop"
    );
    // A concurrent producer of the key does not capture the follower's
    // next claim either: it produces alone.
    let (other, _other_attempt) = claim(&store, &host, &producer);
    let Claim::Produce(_other_lease) = other else {
        panic!("a fresh claim of the key produces it");
    };
    let mut capture = ReadCapture::default();
    let again = store.claim_query(
        &host,
        host.request_flags(),
        &mut follower_attempt,
        &follower,
        &mut capture,
    );
    assert!(
        matches!(again, Claim::Produce(_)),
        "the follower computes the key itself"
    );
}

/// A claim refused the receipt of the stored result computes the key
/// instead, and never subscribes to another task's producer of it: that
/// producer's result is one more result the claimant may not be able to
/// pay for, and waiting on it would move the claimant's trip to wherever
/// that producer happens to be.
#[test]
fn a_recomputing_claim_never_subscribes() {
    let (host, _hash) = keyed_host();
    let store = SemanticGraphStore::new();
    let owner = store.task_registry_for_tests().register_task();
    let recomputer = store.task_registry_for_tests().register_task();

    let (claimed, _owner_attempt) = claim(&store, &host, &owner);
    let Claim::Produce(_lease) = claimed else {
        panic!("the first claim of a cold key produces it");
    };
    let Ok(mut attempt) = store.begin_query_recompute(host.request_flags(), key()) else {
        panic!("an uncancelled recomputation begins");
    };
    let mut capture = ReadCapture::default();
    let claim = store.claim_query(
        &host,
        host.request_flags(),
        &mut attempt,
        &recomputer,
        &mut capture,
    );
    assert!(
        matches!(claim, Claim::Produce(_)),
        "the recomputing claim produces on a flight of its own"
    );
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
            match store.acquire_query(
                &host,
                host.request_flags(),
                key(),
                &mut execution,
                &mut capture,
                false,
                &mut || None,
            ) {
                super::producer::Acquired::Produce(_) => "produce",
                super::producer::Acquired::Read(_) | super::producer::Acquired::Served(_) => "read",
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
