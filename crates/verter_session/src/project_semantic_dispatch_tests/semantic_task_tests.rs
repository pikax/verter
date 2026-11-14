//! The production semantic task — the continuation runtime's drive of a
//! query, which borrows its dispatcher's request state — is `Send` with one
//! exclusive poller: it suspends on another task's producer, moves to
//! another thread while pending, resumes there to the answer a blocking
//! drive gives, and releases every frame, flight and wait it holds when it
//! is released while pending, returning its dispatcher's request state with
//! nothing left open.

use std::sync::Arc;
use std::task::{Poll, Waker};

use verter_type_engine::project_semantic_dispatch::{
    OpenRequestStateForTests, ProjectSemanticDispatch, SemanticTask, TaskOutcomeForTests,
};
use verter_type_engine::resolver_core::fact_validation_port::FactValidation as _;
use verter_type_engine::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId,
    SemanticNodeData, SemanticQueryKey, SemanticQueryValue, ValueRootKey,
};
use verter_type_engine::semantic_query_memo::{Claim, ReadCapture};

const TASK_FILE: &str = "/semantic-task/chain.ts";

/// A chain of constants each initialized from the one before: the `typeof`
/// of the last is a chain of frames, one per constant.
const CHAIN: &str = "const x0 = \"ok\" as const;\n\
                     const x1 = x0;\n\
                     const x2 = x1;\n\
                     const x3 = x2;\n\
                     const x4 = x3;\n\
                     const x5 = x4;\n";

fn chain_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        TASK_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CHAIN),
        crate::FileLanguage::script_ts(),
    );
    // These tests hold semantic producers, after the independent input handoff.
    host.ensure_indexed_ready(TASK_FILE)
        .expect("the chain has indexed input");
    host
}

/// The `typeof` of `name` an initializer naming it reads.
fn typeof_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    name: &str,
) -> SemanticQueryKey {
    dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(
                TASK_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: verter_type_expr::OwnedStr::from(name),
        },
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    )
}

/// The display of the node a read answered.
fn display(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    answer: &QueryResult<SemanticQueryValue>,
) -> String {
    match answer {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => {
            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(dispatch, *node, 0)
        }
        other => panic!("the chain answers a type: {other:?}"),
    }
}

/// Counts its wakes.
#[derive(Default)]
struct CountingWaker(std::sync::atomic::AtomicUsize);

impl std::task::Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn assert_send<T: Send>(_: &T) {}

/// A semantic producer and a scoped cache producer must use the same cycle
/// authority, whichever dependency is registered first. Neither wait leaves
/// a native stack active: the test holds both owned producers explicitly.
#[test]
fn scoped_cache_handoffs_share_the_semantic_wait_graph() {
    use verter_execution::tasks::ExecutionScope;
    use verter_scheduler::scheduler::{
        ScopedCacheNodeError, ScopedCacheNodeRequest, ScopedCacheNodeStep,
    };
    use verter_type_engine::semantic_query_memo::{Joined, Recursion};

    for semantic_waits_first in [false, true] {
        let host = chain_host();
        let graph = host.project_type_store().semantic_graph();
        let registry = graph.task_registry_for_tests();
        let cache_task = registry.register_task();
        let semantic_task = registry.register_task();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let key = typeof_key(&dispatch, "x2");
        let mut capture = ReadCapture::default();
        let mut semantic_claim = graph
            .begin_query_claim(&*host, host.request_flags(), key.clone(), &mut capture)
            .unwrap_or_else(|_| panic!("the semantic producer is cold"));
        let Claim::Produce(semantic_producer) = graph.claim_query(
            &*host,
            host.request_flags(),
            &mut semantic_claim,
            &semantic_task,
            &mut capture,
        ) else {
            panic!("one task owns the semantic producer");
        };
        let request = ScopedCacheNodeRequest {
            cache_id: verter_scheduler::cache_id::SchedulerCacheId(0xC1C1E),
            key_hash: [7; 16],
            view_epoch: 0,
            snapshot_pin_id: verter_scheduler::dag::PinId(1),
            priority: verter_scheduler::stage::Priority::Interactive,
            request_context: None,
        };
        let mut cache_root = {
            let _task = ExecutionScope::install(registry, &cache_task);
            host.scheduler
                .begin_scoped_cache_node::<u64>(request.clone())
                .unwrap()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let granted = verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::ScopedCacheNode,
            Some(std::time::Duration::from_millis(1)),
            |waker| {
                assert!(
                    std::time::Instant::now() < deadline,
                    "cache dispatch stalled"
                );
                cache_root.poll(waker)
            },
        );
        let Ok(ScopedCacheNodeStep::Produce(cache_producer)) = granted else {
            panic!("the cache task receives the sole owned producer");
        };
        assert_send(&cache_producer);
        let mut cache_follower = {
            let _task = ExecutionScope::install(registry, &semantic_task);
            host.scheduler
                .begin_scoped_cache_node::<u64>(request)
                .unwrap()
        };
        let mut dependent_claim = graph
            .begin_query_claim(&*host, host.request_flags(), key, &mut capture)
            .unwrap_or_else(|_| panic!("the semantic producer is still held"));

        let Claim::Subscribed(mut semantic_follower) = graph.claim_query(
            &*host,
            host.request_flags(),
            &mut dependent_claim,
            &cache_task,
            &mut capture,
        ) else {
            panic!("the cache task subscribes to the held semantic producer");
        };
        if semantic_waits_first {
            assert!(semantic_follower
                .poll(
                    &*host,
                    host.request_flags(),
                    &mut dependent_claim,
                    &mut capture,
                    Waker::noop(),
                )
                .is_pending());
            assert_eq!(registry.wait_count_for_tests(), 1);
            assert!(matches!(
                cache_follower.poll(Waker::noop()),
                Poll::Ready(Err(ScopedCacheNodeError::Reentrant))
            ));
        } else {
            assert!(cache_follower.poll(Waker::noop()).is_pending());
            assert_eq!(registry.wait_count_for_tests(), 1);
            assert!(matches!(
                semantic_follower.poll(
                    &*host,
                    host.request_flags(),
                    &mut dependent_claim,
                    &mut capture,
                    Waker::noop(),
                ),
                Poll::Ready(Joined::Recursive(Recursion::WaitCycle))
            ));
        }
        drop(semantic_follower);
        drop(cache_follower);
        assert_eq!(registry.wait_count_for_tests(), 0);
        // Resolving either cycle affects only its dependent subscriber. The
        // surviving cache owner still receives its producer's actual value.
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    cache_producer.run(|_| {
                        let current = ExecutionScope::current(registry).expect("producer task");
                        assert_eq!(current.id(), cache_task.id());
                        41
                    });
                    assert!(ExecutionScope::current(registry).is_none());
                })
                .join()
                .unwrap();
        });
        assert!(matches!(
            cache_root.poll(Waker::noop()),
            Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 41
        ));
        drop(semantic_producer);
        drop(cache_root);
        drop(cache_task);
        drop(semantic_task);
        assert_eq!(graph.wait_graph_counts_for_tests(), (0, 0));
    }
}

#[test]
fn semantic_task_partiality_returns_to_its_lender_without_tainting_the_poller() {
    use verter_type_engine::request_context::{
        current_cold_compute_completeness, ColdComputeCompletenessScope,
    };

    for (source, partial) in [
        ("const y = 1;", false),
        ("const y = undefinedFn(1);", true),
        ("declare const o: any; const y = o.m();", true),
        (
            "declare function f<T>(x: T): T; const y = f(undefinedVal);",
            true,
        ),
    ] {
        for cross_thread in [false, true] {
            for drop_task in [false, true] {
                let host =
                    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
                crate::u6_flow_shape_corpus_tests::upsert(
                    &host,
                    TASK_FILE,
                    &crate::u6_flow_shape_corpus_tests::module_script(source),
                    crate::FileLanguage::script_ts(),
                );
                host.ensure_indexed_ready(TASK_FILE)
                    .expect("partiality is tested after the indexed-input handoff");
                let dispatch = ProjectSemanticDispatch::new(&*host);
                let lender = ColdComputeCompletenessScope::enter();
                dispatch
                    .with_semantic_task_for_tests(typeof_key(&dispatch, "y"), |slot| {
                        let mut poll = || {
                            let poller = ColdComputeCompletenessScope::enter();
                            let waker = Waker::from(Arc::new(CountingWaker::default()));
                            assert!(matches!(
                                slot.as_mut().unwrap().poll_for_tests(&waker),
                                Poll::Ready(TaskOutcomeForTests::Answered(_))
                            ));
                            let polluted = current_cold_compute_completeness().is_partial();
                            poller.discard();
                            if drop_task {
                                drop(slot.take());
                            }
                            polluted
                        };
                        let polluted = if cross_thread {
                            std::thread::scope(|scope| scope.spawn(poll).join().unwrap())
                        } else {
                            poll()
                        };
                        assert!(
                            !polluted,
                            "the poller's compute must stay independent: {source}"
                        );
                    })
                    .expect("the cold root runs as a task");
                let observed = current_cold_compute_completeness().is_partial();
                lender.discard();
                assert_eq!(observed, partial, "lender partiality: {source}, cross_thread={cross_thread}, drop_task={drop_task}");
            }
        }
    }
}

const NOTHING_OPEN: OpenRequestStateForTests = OpenRequestStateForTests {
    active_instantiations: 0,
    open_cost_scopes: 0,
    request_local_open: 0,
    open_obligation_frames: 0,
    collecting_sessions: 0,
    normalizing_carriers: 0,
};

/// A task whose chain reaches a `typeof` another task is producing waits on
/// that producer with every frame of its chain suspended. Moved to another
/// thread, it stays pending there; once the other producer goes away it is
/// woken, resumes on that thread, produces the value itself and answers as
/// a blocking drive on a fresh host does. Its dispatcher gets its request
/// state back with nothing left open.
#[test]
fn a_pending_semantic_task_resumes_on_another_thread_to_the_blocking_answer() {
    let blocking = {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let read = dispatch.execute_read(typeof_key(&dispatch, "x5"));
        let answer = match read.value() {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
            other => panic!("the blocking drive answers a type: {other:?}"),
        };
        display(&dispatch, &answer)
    };
    assert_eq!(blocking, "\"ok\"");

    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    // Another request's task is producing `typeof x2`.
    let other = graph.task_registry_for_tests().register_task();
    let mut capture = ReadCapture::default();
    let Ok(mut attempt) = graph.begin_query_claim(
        &*host,
        host.request_flags(),
        typeof_key(&dispatch, "x2"),
        &mut capture,
    ) else {
        panic!("a cold typeof has no warm answer");
    };
    let Claim::Produce(lease) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut attempt,
        &other,
        &mut capture,
    ) else {
        panic!("the other task produces the cold typeof");
    };

    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let answer = dispatch
        .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
            let task: &mut SemanticTask<'_, '_, _> = slot.as_mut().expect("the task is lent");
            assert_send(task);
            assert!(task.poll_for_tests(&waker).is_pending());
            assert_eq!(
                task.live_frames_for_tests(),
                3,
                "the frames of x5, x4 and x3 wait on x2"
            );
            assert_eq!(
                graph.wait_graph_counts_for_tests().1,
                1,
                "the task waits on the other producer"
            );
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        assert!(
                            task.poll_for_tests(&waker).is_pending(),
                            "moved to another thread, the task still waits"
                        );
                    })
                    .join()
                    .expect("the pending poll returns");
            });
            drop(lease);
            assert!(
                counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
                "the producer going away wakes the task"
            );
            std::thread::scope(|scope| {
                scope
                    .spawn(|| match task.poll_for_tests(&waker) {
                        Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                        Poll::Ready(TaskOutcomeForTests::Stopped) => {
                            panic!("nothing stops the task")
                        }
                        Poll::Pending => panic!("the woken task produces x2 itself"),
                    })
                    .join()
                    .expect("the resumed task answers")
            })
        })
        .expect("the cold root runs as a task");
    assert_eq!(display(&dispatch, &answer), blocking);
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(other);
}

/// A task released while pending — its request abandoned after it was
/// resumed elsewhere — releases its suspended frames through its program:
/// their flights abort, its wait on the other producer goes, and the cost
/// recordings its frames opened close, so the request state it gives back
/// holds nothing more than before it began.
#[test]
fn a_semantic_task_released_while_pending_leaves_nothing_open() {
    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let other = graph.task_registry_for_tests().register_task();
    let mut capture = ReadCapture::default();
    let x2 = typeof_key(&dispatch, "x2");
    let Ok(mut attempt) =
        graph.begin_query_claim(&*host, host.request_flags(), x2.clone(), &mut capture)
    else {
        panic!("a cold typeof has no warm answer");
    };
    let Claim::Produce(lease) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut attempt,
        &other,
        &mut capture,
    ) else {
        panic!("the other task produces the cold typeof");
    };
    let before = dispatch.open_request_state_for_tests();
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    dispatch
        .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            assert!(task.poll_for_tests(&waker).is_pending());
            assert_eq!(task.live_frames_for_tests(), 3);
            assert_eq!(
                graph.resident_flight_keys_for_tests().len(),
                4,
                "the other producer's flight and the task's three"
            );
            std::thread::scope(|scope| {
                scope
                    .spawn(|| assert!(task.poll_for_tests(&waker).is_pending()))
                    .join()
                    .expect("the pending poll returns");
            });
            // The task is left pending: the entry takes it back.
        })
        .expect("the cold root runs as a task");
    assert_eq!(
        dispatch.open_request_state_for_tests(),
        before,
        "the released frames close what they opened"
    );
    assert_eq!(
        graph.resident_flight_keys_for_tests(),
        vec![x2],
        "only the other producer's flight remains"
    );
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(lease);
    drop(other);
    assert!(graph.resident_flight_keys_for_tests().is_empty());
}

/// A pending task dropped on another thread releases its frames, flights
/// and wait there: dropping is the cancellation of a task nobody returns.
#[test]
fn a_pending_semantic_task_dropped_on_another_thread_releases_its_frames() {
    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let other = graph.task_registry_for_tests().register_task();
    let mut capture = ReadCapture::default();
    let x2 = typeof_key(&dispatch, "x2");
    let Ok(mut attempt) =
        graph.begin_query_claim(&*host, host.request_flags(), x2.clone(), &mut capture)
    else {
        panic!("a cold typeof has no warm answer");
    };
    let Claim::Produce(lease) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut attempt,
        &other,
        &mut capture,
    ) else {
        panic!("the other task produces the cold typeof");
    };
    let _request =
        verter_type_engine::request_context::RequestContextGuard::install(budgeted_request(1));
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    dispatch
        .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
            let mut task = slot.take().expect("the task is lent");
            assert!(task.poll_for_tests(&waker).is_pending());
            std::thread::scope(|scope| {
                scope
                    .spawn(move || drop(task))
                    .join()
                    .expect("the task drops on another thread");
            });
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            assert_eq!(graph.resident_flight_keys_for_tests(), vec![x2.clone()]);
            assert_eq!(
                Arc::strong_count(&counter),
                2,
                "dropping a pending task withdraws readiness and cancellation wakers"
            );
        })
        .expect("the cold root runs as a task");
    drop(lease);
    drop(other);
}

/// A conditional over two interned primitives: deciding it asks the
/// free-type-variable summary of each operand, so its drive composes the
/// summaries of `check` and `extends` in the driving dispatcher.
fn primitive_conditional_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    check: verter_type_engine::semantic_query::SemanticNodeId,
    extends: verter_type_engine::semantic_query::SemanticNodeId,
) -> SemanticQueryKey {
    let graph = dispatch.graph_arc();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch: string,
        false_branch: number,
        distributive: false,
        pending: None,
    }
}

/// A later operand demand must not repeat the preparation the decision
/// already completed. Count the actual preparation entry on each poll's
/// thread, and compare its answer with the synchronous entry.
#[test]
fn a_conditional_retains_prepared_operands_across_a_dependency_wait() {
    use verter_type_engine::semantic_query::{
        CacheRead, DeclIdentity, InstantiateKey, PayloadArray, SemanticNodeId,
    };

    let proof = |read: &CacheRead<QueryResult<SemanticNodeId>>| {
        assert!(
            read.walker_diagnostics().is_empty(),
            "the closed conditional has no diagnostics"
        );
        (
            *read.fact_status(),
            read.cache_suppress(),
            read.receipt()
                .priced()
                .expect("the complete decision has a receipt")
                .exclusive(),
        )
    };

    let fixture = || {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            TASK_FILE,
            "type Later<T> = T; export {};",
            crate::FileLanguage::script_ts(),
        );
        assert!(host.ensure_indexed_ready(TASK_FILE).is_some());
        host
    };
    let keys =
        |host: &crate::VerterHost,
         dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>| {
            let graph = dispatch.graph_arc();
            let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            let base = DeclIdentity {
                canonical_id: TASK_FILE.into(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: host.shallow_file_state(TASK_FILE).unwrap().whole_hash,
                decl_name: "Later".into(),
            };
            let check = graph.intern_node(SemanticNodeData::InstantiationRef {
                base,
                args: PayloadArray::from(vec![number]),
            });
            let dependency = SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(
                    TASK_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    "Later",
                ),
                Arc::from([number]),
                dispatch.instantiate_context_for(
                    TASK_FILE,
                    ProjectionReductionContext::structural_transit(),
                ),
            ));
            (
                primitive_conditional_key(dispatch, check, number),
                dependency,
            )
        };
    let (blocking, blocking_proof) = {
        let host = fixture();
        let view = host.resolver_store_view_read().into_owned_view();
        let ctx = crate::resolver_core::HostResolverContext::new(
            &host,
            &view,
            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        let _builder = dispatch.enter_construction_scope();
        let read = dispatch.execute_read(keys(&host, &dispatch).0);
        let QueryResult::Value(node) = read.value() else {
            panic!("the synchronous conditional answers a type: {read:?}");
        };
        (
            display(
                &dispatch,
                &QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
            ),
            proof(&read),
        )
    };
    assert_eq!(blocking, "string");

    let host = fixture();
    let view = host.resolver_store_view_read().into_owned_view();
    let ctx = crate::resolver_core::HostResolverContext::new(
        &host,
        &view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    let _builder = dispatch.enter_construction_scope();
    let graph = host.project_type_store().semantic_graph();
    let (key, dependency) = keys(&host, &dispatch);
    let (other, lease) = hold_producer(&host, graph, dependency);
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let preparations = || {
        ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::conditional_operand_preparations_for_tests()
    };
    let (answer, prepared) = dispatch
        .with_semantic_task_for_tests(key.clone(), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            let ready = std::sync::Barrier::new(2);
            let (pending, prepared) = std::thread::scope(|scope| {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let task = &mut *task;
                let waker = &waker;
                let ready = &ready;
                let poller = scope.spawn(move || {
                    ready.wait();
                    let before = preparations();
                    let pending = task.poll_read_for_tests(waker);
                    let _ = tx.send((pending, preparations() - before));
                });
                ready.wait();
                let observed = rx.recv_timeout(std::time::Duration::from_secs(5));
                // Release and join even when a planted blocking poll misses
                // the deadline or its worker panics.
                drop(lease);
                poller
                    .join()
                    .expect("the initial poll returns after release");
                observed.expect("a held dependency must yield before release")
            });
            assert!(
                pending.is_pending(),
                "the held operand must suspend: {pending:?}"
            );
            assert_eq!(
                prepared, 1,
                "preparation finishes before the dependency waits"
            );
            let before = preparations();
            let answer = task.poll_read_for_tests(&waker);
            let resumed = preparations() - before;
            let Poll::Ready(Ok(answer)) = answer else {
                panic!("the released conditional answers: {answer:?}");
            };
            let answer = answer.map_value(|value| match value {
                QueryResult::Value(SemanticQueryValue::TypeNode(node)) => QueryResult::Value(node),
                other => panic!("the conditional answers a type: {other:?}"),
            });
            (answer, prepared + resumed)
        })
        .expect("the cold conditional runs as a task");
    let QueryResult::Value(node) = answer.value() else {
        panic!("the resumed conditional answers a type: {answer:?}");
    };
    assert_eq!(
        display(
            &dispatch,
            &QueryResult::Value(SemanticQueryValue::TypeNode(*node))
        ),
        blocking
    );
    assert_eq!(
        prepared, 1,
        "completed operand preparation is never replayed"
    );
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(
        proof(&answer),
        blocking_proof,
        "the resumed decision preserves diagnostics, completeness and exclusive logical cost"
    );
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(other);
}

/// A distributed conditional suspends at a held member without blocking its
/// poller or revisiting a completed sibling. Resume on another thread and
/// compare the full read with an independent cold synchronous decision.
#[test]
fn semantic_poll_contract_distributed_conditional_retains_completed_members_across_a_wait() {
    use verter_type_engine::semantic_query::{
        CacheRead, DeclIdentity, InstantiateKey, LiteralValue, PayloadArray, SemanticNodeId,
    };

    let fixture = || {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            TASK_FILE,
            "type Later<T> = T; export {};",
            crate::FileLanguage::script_ts(),
        );
        assert!(host.ensure_indexed_ready(TASK_FILE).is_some());
        host
    };
    let keys =
        |host: &crate::VerterHost,
         dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>| {
            let graph = host.project_type_store().semantic_graph();
            let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
            let [yes, no] = [true, false].map(|value| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(value)))
            });
            let SemanticQueryKey::Conditional {
                true_branch,
                false_branch,
                ..
            } = primitive_conditional_key(dispatch, yes, yes)
            else {
                unreachable!();
            };
            let binder = graph.alloc_infer_binder_id();
            let infer = graph.intern_node(SemanticNodeData::Infer {
                name: "U".into(),
                binder: binder.clone(),
                constraint: None,
            });
            let inferred = graph.intern_node(SemanticNodeData::InferRef {
                name: "U".into(),
                binder,
            });
            let late = graph.intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: TASK_FILE.into(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: host.shallow_file_state(TASK_FILE).unwrap().whole_hash,
                    decl_name: "Later".into(),
                },
                args: PayloadArray::from(vec![inferred]),
            });
            let selected = graph.intern_node(SemanticNodeData::Conditional {
                check: late,
                extends: yes,
                true_branch_ref: true_branch,
                false_branch_ref: false_branch,
                distributive: false,
                pending: None,
            });
            // boolean extends infer U ? (Later<U> extends true ? string : number) : number
            let key = SemanticQueryKey::Conditional {
                check: boolean,
                extends: infer,
                true_branch: selected,
                false_branch,
                distributive: true,
                pending: None,
            };
            let held = SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(
                    TASK_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    "Later",
                ),
                Arc::from([no]),
                dispatch.instantiate_context_for(
                    TASK_FILE,
                    ProjectionReductionContext::structural_transit(),
                ),
            ));
            (key, held)
        };
    let proof = |read: &CacheRead<QueryResult<SemanticNodeId>>| {
        assert!(
            read.walker_diagnostics().is_empty(),
            "the closed union has no diagnostics"
        );
        (
            *read.fact_status(),
            read.cache_suppress(),
            read.dep_signature().clone(),
            read.receipt()
                .priced()
                .expect("a decision receipt")
                .exclusive(),
        )
    };
    let selections = || {
        ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::conditional_branch_selections_for_tests()
    };
    let (expected, expected_proof, expected_selections) = {
        let host = fixture();
        let view = host.resolver_store_view_read().into_owned_view();
        let ctx = crate::resolver_core::HostResolverContext::new(
            &host,
            &view,
            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let dispatch = ProjectSemanticDispatch::new(&ctx);
        let _builder = dispatch.enter_construction_scope();
        let before = selections();
        let read = dispatch.execute_read(keys(&host, &dispatch).0);
        let QueryResult::Value(node) = read.value() else {
            panic!("the cold synchronous conditional must answer: {read:?}");
        };
        (
            display(
                &dispatch,
                &QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
            ),
            proof(&read),
            selections() - before,
        )
    };
    assert_eq!(expected, "Union(number | string)");
    assert_eq!(
        expected_selections, 4,
        "each boolean member selects its inferred branch and its nested branch once"
    );
    let host = fixture();
    let view = host.resolver_store_view_read().into_owned_view();
    let ctx = crate::resolver_core::HostResolverContext::new(
        &host,
        &view,
        Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );
    let mut dispatch = ProjectSemanticDispatch::new(&ctx);
    let _builder = dispatch.enter_construction_scope();
    let graph = host.project_type_store().semantic_graph();
    let (key, held) = keys(&host, &dispatch);
    let (other, lease) = hold_producer(&host, graph, held);
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let folds = || {
        ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::conditional_distributed_members_for_tests()
    };
    let mut entry = dispatch.poll_query(key.clone());
    let (pending, before_release, selections_before_release) = std::thread::scope(|scope| {
        let (release, released) = std::sync::mpsc::channel::<()>();
        let holder = scope.spawn(move || {
            let _ = released.recv_timeout(std::time::Duration::from_secs(5));
            drop(lease);
        });
        let before = folds();
        let before_selections = selections();
        let pending = entry.poll(&waker);
        let completed = folds() - before;
        let completed_selections = selections() - before_selections;
        let _ = release.send(());
        holder.join().expect("the held producer releases");
        (pending, completed, completed_selections)
    });
    assert!(
        pending.is_pending(),
        "the held member must return Pending before release"
    );
    assert_eq!(before_release, 1, "the first sibling already completed");
    let task = entry.task().expect("the pending decision owns its task");
    assert_send(task);
    let (completed, resumed_selections) = std::thread::scope(|scope| {
        let waker = &waker;
        scope
            .spawn(move || {
                let before_folds = folds();
                let before_selections = selections();
                assert!(task.poll(waker).is_ready(), "the released task completes");
                (folds() - before_folds, selections() - before_selections)
            })
            .join()
            .expect("the task resumes on another thread")
    });
    let Poll::Ready(answer) = entry.poll(&waker) else {
        panic!("the completed task delivers through the production entry");
    };
    drop(entry);
    let completed = before_release + completed;
    assert_eq!(completed, 2, "each member is folded exactly once");
    assert_eq!(
        selections_before_release + resumed_selections,
        expected_selections,
        "completed relation/inference selection is retained while its winner waits"
    );
    assert_eq!(display(&dispatch, answer.value()), expected);
    assert!(
        !answer.result_is_partial(),
        "the union is complete: {answer:?}"
    );
    assert!(answer.walker_diagnostics().is_empty());
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(other);
    let synchronous = dispatch.execute_read(key);
    let answer = answer.map_value(|value| match value {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => QueryResult::Value(node),
        other => panic!("a conditional answers a type: {other:?}"),
    });
    assert_eq!(
        proof(&answer),
        expected_proof,
        "suspension preserves the cold answer's evidence and exclusive logical work",
    );
    let (QueryResult::Value(answered), QueryResult::Value(expected)) =
        (answer.value(), synchronous.value())
    else {
        panic!("both conditional entries must answer a type");
    };
    assert_eq!(answered, expected);
    assert_eq!(answer.fact_status(), synchronous.fact_status());
    assert_eq!(answer.cache_suppress(), synchronous.cache_suppress());
    assert_eq!(
        answer.walker_diagnostics(),
        synchronous.walker_diagnostics()
    );
    assert_eq!(answer.dep_signature(), synchronous.dep_signature());
    assert_eq!(
        answer
            .receipt()
            .priced()
            .expect("a resumed receipt")
            .exclusive(),
        synchronous
            .receipt()
            .priced()
            .expect("a synchronous receipt")
            .exclusive(),
    );
}

/// The free-type-variable summaries a request composes are request state
/// like every other the semantic task borrows: the task's dispatcher starts
/// from the summaries its lender already composed, the summaries its own
/// drive composes — on whatever thread resumes it — come back to the lender
/// with the rest of the borrowed state, and a read afterwards reuses them
/// instead of recomposing them.
#[test]
fn a_semantic_task_inherits_and_returns_the_free_type_variable_summaries() {
    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = dispatch.graph_arc();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let read = dispatch.execute_read(primitive_conditional_key(&dispatch, number, string));
    assert!(
        matches!(read.value(), QueryResult::Value(_)),
        "a decided conditional answers a value: {read:?}"
    );
    let composed = dispatch.free_type_variable_summaries_for_tests();
    assert!(
        composed >= 2,
        "deciding a conditional composes a summary per operand"
    );

    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let resume_answered = |slot: &mut Option<
        SemanticTask<'_, '_, crate::resolver_core::HostCapabilities>,
    >,
                           inherited: usize| {
        let task = slot.as_mut().expect("the task is lent");
        assert_eq!(
            task.free_type_variable_summaries_for_tests(),
            inherited,
            "the lent state carries the composed summaries"
        );
        std::thread::scope(|scope| {
            scope
                .spawn(|| match task.poll_for_tests(&waker) {
                    Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                    Poll::Ready(TaskOutcomeForTests::Stopped) => panic!("nothing stops the task"),
                    Poll::Pending => panic!("a decided conditional answers at once"),
                })
                .join()
                .expect("the resumed task answers")
        })
    };
    // A conditional over the operands the request already summarised: the
    // task inherits their summaries and composes nothing new.
    dispatch
        .with_semantic_task_for_tests(
            primitive_conditional_key(&dispatch, string, number),
            |slot| resume_answered(slot, composed),
        )
        .expect("the cold conditional runs as a task");
    assert_eq!(
        dispatch.free_type_variable_summaries_for_tests(),
        composed,
        "a conditional over already-summarised operands composes nothing new"
    );

    // A conditional over a fresh operand: the one new summary its drive
    // composes on the other thread comes back with the request state.
    let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    dispatch
        .with_semantic_task_for_tests(
            primitive_conditional_key(&dispatch, boolean, number),
            |slot| resume_answered(slot, composed),
        )
        .expect("the cold conditional runs as a task");
    let returned = dispatch.free_type_variable_summaries_for_tests();
    assert!(
        returned > composed,
        "the summaries the task composed come back with the request state"
    );

    dispatch.execute_read(primitive_conditional_key(&dispatch, number, string));
    assert_eq!(
        dispatch.free_type_variable_summaries_for_tests(),
        returned,
        "the returned summaries are reused, not recomposed"
    );
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
}

/// A task abandoned while it holds the lent state — left pending on another
/// producer and dropped on another thread without being returned — gives
/// its lender the summaries it inherited back: nothing the request already
/// composed is lost with the abandoned task, and the lender holds nothing
/// open afterwards.
#[test]
fn an_abandoned_semantic_task_returns_the_summaries_it_inherited() {
    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let number = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let read = dispatch.execute_read(primitive_conditional_key(&dispatch, number, string));
    assert!(
        matches!(read.value(), QueryResult::Value(_)),
        "a decided conditional answers a value: {read:?}"
    );
    let composed = dispatch.free_type_variable_summaries_for_tests();
    assert!(
        composed >= 2,
        "deciding a conditional composes a summary per operand"
    );

    let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    dispatch
        .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
            let mut task = slot.take().expect("the task is lent");
            assert_eq!(
                task.free_type_variable_summaries_for_tests(),
                composed,
                "the lent state carries the composed summaries"
            );
            assert!(
                task.poll_for_tests(&waker).is_pending(),
                "the task waits on the other producer"
            );
            std::thread::scope(|scope| {
                scope
                    .spawn(move || drop(task))
                    .join()
                    .expect("the task drops on another thread");
            });
        })
        .expect("the cold root runs as a task");
    assert_eq!(
        dispatch.free_type_variable_summaries_for_tests(),
        composed,
        "the abandoned task returns the summaries it inherited"
    );
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    drop(lease);
    drop(other);
}

/// Another task's producer of `key`, held until the lease drops.
pub(super) fn hold_producer<'g>(
    host: &crate::VerterHost,
    graph: &'g verter_type_engine::semantic_query_memo::SemanticGraphStore,
    key: SemanticQueryKey,
) -> (
    verter_execution::tasks::ExecutionTask,
    verter_type_engine::semantic_query_memo::ProducerLease<'g>,
) {
    hold_producer_with(host, graph, key, ReadCapture::default())
}

/// [`hold_producer`] for a key checked at the site `capture` names.
fn hold_producer_with<'g>(
    host: &crate::VerterHost,
    graph: &'g verter_type_engine::semantic_query_memo::SemanticGraphStore,
    key: SemanticQueryKey,
    mut capture: ReadCapture<'_>,
) -> (
    verter_execution::tasks::ExecutionTask,
    verter_type_engine::semantic_query_memo::ProducerLease<'g>,
) {
    let other = graph.task_registry_for_tests().register_task();
    let Ok(mut attempt) = graph.begin_query_claim(host, host.request_flags(), key, &mut capture)
    else {
        panic!("a cold typeof has no warm answer");
    };
    let Claim::Produce(lease) = graph.claim_query(
        host,
        host.request_flags(),
        &mut attempt,
        &other,
        &mut capture,
    ) else {
        panic!("the other task produces the cold typeof");
    };
    (other, lease)
}

/// A request of its own, with an operation budget its spend is charged to.
fn budgeted_request(id: u64) -> Arc<verter_type_engine::request_context::RequestContext> {
    verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
        id,
        Arc::from(TASK_FILE),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        1_000_000,
    )
}

/// Run `x5`'s task with `x2` held by another producer: one poll here, then
/// the producer goes away and the task finishes on another thread or here.
fn drive_x5_across_a_wait(
    host: &Arc<crate::VerterHost>,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    elsewhere: bool,
) -> QueryResult<SemanticQueryValue> {
    let graph = host.project_type_store().semantic_graph();
    let (other, lease) = hold_producer(host, graph, typeof_key(dispatch, "x2"));
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let mut lease = Some(lease);
    let answer = dispatch
        .with_semantic_task_for_tests(typeof_key(dispatch, "x5"), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            assert!(task.poll_for_tests(&waker).is_pending());
            drop(lease.take());
            let mut finish = || match semantic_poll_contract::poll_until_ready(|waker| {
                task.poll_for_tests(waker)
            }) {
                TaskOutcomeForTests::Answered(answer) => answer,
                TaskOutcomeForTests::Stopped => panic!("nothing stops the task"),
            };
            if elsewhere {
                std::thread::scope(|scope| {
                    scope
                        .spawn(finish)
                        .join()
                        .expect("the resumed task answers")
                })
            } else {
                finish()
            }
        })
        .expect("the cold root runs as a task");
    drop(other);
    answer
}

/// A task suspended under a scheduler job that two requests share keeps
/// observing that job's cancellation on whatever thread resumes it: the
/// first requester cancelling does not stop work the second still needs,
/// even on a thread with no job installed, where the task's own request
/// is the cancelled one.
#[test]
fn a_pending_semantic_task_keeps_its_jobs_cancellation_on_another_thread() {
    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));
    let first = budgeted_request(1);
    let second = budgeted_request(2);
    let job = verter_execution::cancellation::CancellationToken::aggregate();
    let _first_owner = job
        .register_owner(Some(first.cancellation_token()))
        .expect("the live job accepts the first requester");
    let _second_owner = job
        .register_owner(Some(second.cancellation_token()))
        .expect("the live job accepts the second requester");
    let _job = verter_execution::cancellation::JobCancellationGuard::install(job.clone());
    let _request =
        verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&first));
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let mut lease = Some(lease);
    let answer = dispatch
        .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            assert!(task.poll_for_tests(&waker).is_pending());
            first.cancel();
            assert!(
                !job.is_cancelled(),
                "the second requester keeps the job live"
            );
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        assert!(
                            task.poll_for_tests(&waker).is_pending(),
                            "the job, not the cancelled first request, decides: the task still waits"
                        );
                    })
                    .join()
                    .expect("the pending poll returns");
            });
            drop(lease.take());
            std::thread::scope(|scope| {
                scope
                    .spawn(|| match task.poll_for_tests(&waker) {
                        Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                        Poll::Ready(TaskOutcomeForTests::Stopped) => {
                            panic!("a live job does not stop the task")
                        }
                        Poll::Pending => panic!("the woken task produces x2 itself"),
                    })
                    .join()
                    .expect("the resumed task answers")
            })
        })
        .expect("the cold root runs as a task");
    assert_eq!(display(&dispatch, &answer), "\"ok\"");
    drop(other);
}

/// Cancellation wakes a suspended task without its unrelated producer
/// completing. The same registration follows the exclusive poller to a
/// different thread and is withdrawn when the task stops.
#[test]
fn pending_semantic_task_cancellation_wakes_without_producer_release() {
    use verter_execution::cancellation::{CancellationToken, JobCancellationGuard};
    use verter_type_engine::request_context::RequestContextGuard;

    for (request_installed, shared_job, withdraw_owner) in [
        (true, false, false),
        (false, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let x2 = typeof_key(&dispatch, "x2");
        let (other, lease) = hold_producer(&host, graph, x2.clone());
        let request = budgeted_request(1);
        let sibling = budgeted_request(2);
        let job = if shared_job {
            CancellationToken::aggregate()
        } else {
            CancellationToken::new()
        };
        let first_owner = shared_job.then(|| {
            job.register_owner(Some(request.cancellation_token()))
                .expect("the job accepts the first owner")
        });
        let second_owner = shared_job.then(|| {
            job.register_owner(Some(sibling.cancellation_token()))
                .expect("the job accepts the second owner")
        });
        let job_guard = JobCancellationGuard::install(job.clone());
        let request_guard =
            request_installed.then(|| RequestContextGuard::install(Arc::clone(&request)));
        let first = Arc::new(CountingWaker::default());
        let first_waker = Waker::from(Arc::clone(&first));
        let latest = Arc::new(CountingWaker::default());
        let latest_waker = Waker::from(Arc::clone(&latest));
        dispatch
            .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
                let task = slot.as_mut().expect("the task is lent");
                assert!(task.poll_for_tests(&first_waker).is_pending());
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| assert!(task.poll_for_tests(&latest_waker).is_pending()))
                        .join()
                        .expect("the pending task moves to its next poller");
                });
                if shared_job {
                    request.cancel();
                    assert_eq!(latest.0.load(std::sync::atomic::Ordering::SeqCst), 0);
                    if withdraw_owner {
                        drop(second_owner);
                    } else {
                        sibling.cancel();
                    }
                } else if request_installed {
                    // An ownerless job's token does not decide this request.
                    job.cancel();
                    assert!(task.poll_for_tests(&latest_waker).is_pending());
                    latest.0.store(0, std::sync::atomic::Ordering::SeqCst);
                    request.cancel();
                } else {
                    job.cancel();
                }
                assert_eq!(first.0.load(std::sync::atomic::Ordering::SeqCst), 0);
                assert!(
                    latest.0.load(std::sync::atomic::Ordering::SeqCst) > 0,
                    "cancellation wakes the current poller before the producer is released"
                );
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| {
                            assert!(matches!(
                                task.poll_for_tests(&latest_waker),
                                Poll::Ready(TaskOutcomeForTests::Stopped)
                            ));
                        })
                        .join()
                        .expect("the woken task stops on another thread");
                });
                assert_eq!(task.live_frames_for_tests(), 0);
                assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
                assert_eq!(graph.resident_flight_keys_for_tests(), vec![x2.clone()]);
                assert_eq!(
                    Arc::strong_count(&latest),
                    2,
                    "a terminal task retains neither readiness nor cancellation wakers"
                );
            })
            .expect("the cold root runs as a task");
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        drop(first_owner);
        drop(request_guard);
        drop(job_guard);
        drop(lease);
        drop(other);
        let read = dispatch.execute_read(typeof_key(&dispatch, "x5"));
        let QueryResult::Value(node) = read.value() else {
            panic!("cancellation cannot publish a reusable answer: {read:?}");
        };
        assert_eq!(
            display(
                &dispatch,
                &QueryResult::Value(SemanticQueryValue::TypeNode(*node))
            ),
            "\"ok\""
        );
        assert_eq!(
            read.fact_status(),
            &verter_type_engine::semantic_query::FactStatus::Complete
        );
    }
}

/// A task pending on another task's held producer is woken by the
/// cancellation that stops it — its request's, or its job's once the last
/// requester cancels — with no timer and the producer still held. Its next
/// poll stops it, leaving nothing open and no wait behind; a requester
/// cancelling while a sibling keeps the job live wakes nothing. The held
/// producer is untouched and still finishes for its own owner.
#[test]
fn a_pending_semantic_task_is_woken_by_the_cancellation_that_stops_it() {
    for under_job in [false, true] {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));
        let first = budgeted_request(1);
        let second = budgeted_request(2);
        let job = verter_execution::cancellation::CancellationToken::aggregate();
        let owners = under_job.then(|| {
            (
                job.register_owner(Some(first.cancellation_token()))
                    .unwrap(),
                job.register_owner(Some(second.cancellation_token()))
                    .unwrap(),
            )
        });
        let _job = under_job
            .then(|| verter_execution::cancellation::JobCancellationGuard::install(job.clone()));
        let _request =
            verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&first));
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        dispatch
            .with_semantic_task_for_tests(typeof_key(&dispatch, "x5"), |slot| {
                let task = slot.as_mut().expect("the task is lent");
                assert!(task.poll_for_tests(&waker).is_pending());
                let before = wakes();
                first.cancel();
                if under_job {
                    assert_eq!(wakes(), before, "the second requester keeps the job live");
                    assert!(task.poll_for_tests(&waker).is_pending());
                    second.cancel();
                }
                assert!(
                    wakes() > before,
                    "the stopping cancellation wakes the pending task (under_job={under_job})"
                );
                assert!(matches!(
                    task.poll_for_tests(&waker),
                    Poll::Ready(TaskOutcomeForTests::Stopped)
                ));
            })
            .expect("the cold root runs as a task");
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "no wait is left");
        drop(lease);
        drop(owners);
        drop(other);
    }
}

/// The work, spend and paid computations a task's frames charge on
/// another thread land in the component-meta compute that lent it the
/// request, exactly as when the task finishes on its own thread.
#[test]
fn a_semantic_task_resumed_on_another_thread_charges_the_compute_that_lent_it() {
    let charged = |elsewhere: bool| {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _request =
            verter_type_engine::request_context::RequestContextGuard::install(budgeted_request(1));
        let ((answer, operations, paid), usage) =
            verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(
                || {
                    verter_type_engine::request_budget::with_request_spend_capture_for_tests(|| {
                        drive_x5_across_a_wait(&host, &dispatch, elsewhere)
                    })
                },
            );
        assert_eq!(display(&dispatch, &answer), "\"ok\"");
        (usage.work, usage.bytes, operations, paid)
    };
    let here = charged(false);
    assert!(
        here.0 > 0 && here.2 > 0,
        "finishing the chain charges work and spends operations: {here:?}"
    );
    assert_eq!(
        charged(true),
        here,
        "finishing on another thread charges the lending compute the same"
    );
}

/// A task dropped instead of returned leaves the request state it
/// borrowed for its lender, which takes it back: the connected demand's
/// charges and the dispatcher's open state are what returning it leaves.
#[test]
fn a_semantic_task_dropped_without_returning_gives_its_lender_the_request_state() {
    let charged = |drop_task: bool, generic: bool| {
        let host = if generic { call_host() } else { chain_host() };
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let key = if generic {
            call_result_key(&dispatch)
        } else {
            typeof_key(&dispatch, "x5")
        };
        let answer = dispatch
            .with_semantic_task_for_tests(key, |slot| {
                let answer = match slot
                    .as_mut()
                    .expect("the task is lent")
                    .poll_for_tests(&waker)
                {
                    Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                    other => panic!("a root nothing else produces answers at once: {other:?}"),
                };
                let live_nodes = dispatch.graph().node_count();
                if drop_task {
                    let task = slot.take().expect("the task is lent");
                    std::thread::scope(|scope| {
                        scope
                            .spawn(move || drop(task))
                            .join()
                            .expect("the task drops on another thread");
                    });
                }
                assert_eq!(
                    dispatch.graph().node_count(),
                    live_nodes,
                    "construction nodes stay alive until the lender reclaims its state"
                );
                answer
            })
            .expect("the cold root runs as a task");
        assert_eq!(
            display(&dispatch, &answer),
            if generic { "Union(2 | 1)" } else { "\"ok\"" }
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        dispatch.connected_demand().work_used_for_tests()
    };
    for generic in [false, true] {
        let returned = charged(false, generic);
        assert!(returned > 0, "the drive charges its connected demand");
        assert_eq!(
            charged(true, generic),
            returned,
            "a dropped task's charges reach its lender"
        );
    }
}

const CALL_FILE: &str = "/semantic-task/call.ts";

/// A generic call whose second argument relates over an alias both its
/// argument's type and its parameter's name, each argument inferring its
/// own literal for `T`.
const CALL: &str = "type Box<T> = { v: T };\n\
                    declare function pick<T>(a: T, b: Box<T>): T;\n\
                    declare const box: Box<2>;\n\
                    const r = pick(1, box);\n";

fn call_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        CALL_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CALL),
        crate::FileLanguage::script_ts(),
    );
    host.ensure_indexed_ready(CALL_FILE)
        .expect("the call has indexed input before its inference is held");
    host
}

/// The `typeof` of the call's result.
fn call_result_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
) -> SemanticQueryKey {
    dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(
                CALL_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: verter_type_expr::OwnedStr::from("r"),
        },
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    )
}

/// Lend the call result's task with the demands its call's walk stops on
/// held by another task, poll it until the walk waits with its collector
/// holding the first argument's inference, and hand it to `then`.
fn suspend_the_call<R>(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    waker: &Waker,
    then: impl FnOnce(&mut Option<SemanticTask<'_, '_, crate::resolver_core::HostCapabilities>>) -> R,
) -> R {
    let _ = verter_type_engine::project_semantic_dispatch::take_call_suspensions_for_tests();
    dispatch
        .with_semantic_task_for_tests(call_result_key(dispatch), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            task.hold_call_needs_for_tests();
            assert!(
                task.poll_for_tests(waker).is_pending(),
                "the call's walk waits on another task's producer"
            );
            let suspensions =
                verter_type_engine::project_semantic_dispatch::take_call_suspensions_for_tests();
            let [suspension] = suspensions.as_slice() else {
                panic!("the call's walk suspends once: {suspensions:?}");
            };
            assert!(
                suspension
                    .needs
                    .iter()
                    .all(|need| matches!(need, SemanticQueryKey::Instantiate(_))),
                "it waits on the instantiations relating `box` to `Box<T>` unwraps: {suspension:?}"
            );
            assert_eq!(
                suspension.collected_candidates, 1,
                "its collector holds what the first argument inferred"
            );
            then(slot)
        })
        .expect("the cold root runs as a task")
}

/// A generic call whose argument relation needs instantiations another task
/// is producing suspends with its inference unfinished: the walk stops
/// before relating `box` to `Box<T>`, its collector holding what `1`
/// inferred for `T`, out of the transaction while the task waits. Moved to
/// another thread, it stays pending; once the producers go away it resumes
/// there with the same collector — `T` still holds `1` beside the `2` the
/// second argument infers — and answers as an uninterrupted resolution
/// does, leaving nothing open.
#[test]
fn a_generic_call_suspends_with_its_inference_open_and_resumes_to_the_uninterrupted_answer() {
    let uninterrupted = {
        let host = call_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let read = dispatch.execute_read(call_result_key(&dispatch));
        let answer = match read.value() {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
            other => panic!("the uninterrupted call answers a type: {other:?}"),
        };
        display(&dispatch, &answer)
    };
    assert_eq!(
        uninterrupted, "Union(2 | 1)",
        "`pick(1, box)` infers `T` from both arguments: what the first deposited survives the second"
    );

    let host = call_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let answer = suspend_the_call(&dispatch, &waker, |slot| {
        let task = slot.as_mut().expect("the task is lent");
        assert!(
            task.live_frames_for_tests() >= 2,
            "the call's frame waits under the `typeof` that reached it"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 1);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    assert!(
                        task.poll_for_tests(&waker).is_pending(),
                        "moved to another thread, the call still waits"
                    );
                })
                .join()
                .expect("the pending poll returns");
        });
        assert!(task.release_held_call_needs_for_tests() > 0);
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producers going away wake the task"
        );
        std::thread::scope(|scope| {
            scope
                .spawn(|| match task.poll_for_tests(&waker) {
                    Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                    Poll::Ready(TaskOutcomeForTests::Stopped) => panic!("nothing stops the task"),
                    Poll::Pending => panic!("the woken call produces its operands itself"),
                })
                .join()
                .expect("the resumed task answers")
        })
    });
    assert!(
        verter_type_engine::project_semantic_dispatch::take_call_suspensions_for_tests().is_empty(),
        "the resumed walk relates over answered operands"
    );
    assert_eq!(display(&dispatch, &answer), uninterrupted);
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    assert!(graph.resident_flight_keys_for_tests().is_empty());
}

/// A call suspended with its inference open, then released while pending —
/// its request abandoned, or the task dropped on another thread — releases
/// its collector, its executor frame, its frames' flights and its wait: no
/// session is left that could still collect, no obligation frame stays
/// open, and the request state its lender gets back holds nothing open.
#[test]
fn a_call_released_while_suspended_with_its_inference_open_leaves_nothing_open() {
    for dropped_elsewhere in [false, true] {
        let host = call_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        suspend_the_call(&dispatch, &waker, |slot| {
            if dropped_elsewhere {
                let task = slot.take().expect("the task is lent");
                std::thread::scope(|scope| {
                    scope
                        .spawn(move || drop(task))
                        .join()
                        .expect("the task drops on another thread");
                });
            }
            // Otherwise the task is left pending: the entry takes it back.
        });
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "the released call closes what it opened (dropped elsewhere: {dropped_elsewhere})"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(
            graph.resident_flight_keys_for_tests().is_empty(),
            "the call's flights and the held producers go"
        );
        // The request goes on: the call resolves afresh to its answer.
        let read = dispatch.execute_read(call_result_key(&dispatch));
        let answer = match read.value() {
            QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
            other => panic!("the call resolves after the release: {other:?}"),
        };
        assert_eq!(display(&dispatch, &answer), "Union(2 | 1)");
    }
}

const CONTEXTUAL_FILE: &str = "/semantic-task/contextual.ts";

/// A body whose generic call infers `T` from its first argument, then types
/// its callback under `(x: T) => U` — the candidate's collector held while
/// the callback types — and infers `U` from what the callback returns.
const CONTEXTUAL: &str = "type Box<T> = { v: T };\n\
                          declare function both<T, U>(seed: T, f: (x: T) => U): [T, U];\n\
                          declare const late: Box<3>;\n\
                          export function f() {\n\
                          return both(1, (x) => late);\n\
                          }\n";

fn contextual_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        CONTEXTUAL_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CONTEXTUAL),
        crate::FileLanguage::script_ts(),
    );
    host
}

/// The return of `f`, evaluated as a machinery root.
fn contextual_return_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
) -> SemanticQueryKey {
    SemanticQueryKey::FlowReturn(Box::new(
        verter_type_engine::semantic_query::FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                verter_type_expr::OwnedStr::from(CONTEXTUAL_FILE),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                verter_type_expr::OwnedStr::from("f"),
                verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: verter_type_expr::OwnedSlice::from(Vec::new().into_boxed_slice()),
            context: dispatch.flow_return_context_for(CONTEXTUAL_FILE),
            demand: verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
            input: verter_type_engine::semantic_query::FlowInputContext::empty(),
            result_contract:
                verter_type_engine::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
        },
    ))
}

/// The display of the return a flow evaluation answered.
fn display_return(host: &crate::VerterHost, answer: &QueryResult<SemanticQueryValue>) -> String {
    match answer {
        QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => {
            let expr = host
                .project_node_to_type_expr_for_test(result.return_type())
                .expect("the return projects to a type");
            crate::resolver_core::surface_projector::render_type_expr_display(&expr)
                .expect("the return displays")
        }
        other => panic!("the body answers a return: {other:?}"),
    }
}

/// The return of `f`, evaluated through the query entry.
fn execute_return(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
) -> QueryResult<SemanticQueryValue> {
    use verter_type_engine::semantic_query::SemanticQueryApi as _;
    match dispatch.execute(contextual_return_key(dispatch)) {
        QueryResult::Value(output) => QueryResult::Value(output.value),
        other => panic!("the body answers a return: {other:?}"),
    }
}

fn contextual_routes(
    host: &crate::VerterHost,
) -> Vec<verter_type_engine::project_semantic_dispatch::ContextualRouteEventForTests> {
    std::mem::take(
        &mut *host
            .flow_fault_injection
            .contextual_routes
            .lock()
            .expect("the observer is never poisoned"),
    )
}

/// Lend `f`'s return as a task with the demands its contextual callback
/// stops on held by another task, poll it until it waits with the call's
/// collector held, and hand it to `then` with that collector's session.
fn suspend_the_contextual_call<R>(
    host: &crate::VerterHost,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    waker: &Waker,
    then: impl FnOnce(
        &mut Option<SemanticTask<'_, '_, crate::resolver_core::HostCapabilities>>,
        u64,
    ) -> R,
) -> R {
    use verter_type_engine::project_semantic_dispatch::ContextualRouteEventForTests as Event;
    let _ = contextual_routes(host);
    dispatch
        .with_semantic_task_for_tests(contextual_return_key(dispatch), |slot| {
            let task = slot.as_mut().expect("the task is lent");
            task.hold_call_needs_for_tests();
            assert!(
                task.poll_for_tests(waker).is_pending(),
                "the callback's read waits on another task's producer"
            );
            let events = contextual_routes(host);
            let [Event::Held { session, collected }] = events.as_slice() else {
                panic!("the call's route is held once, collector included: {events:?}");
            };
            assert_eq!(
                *collected, 1,
                "its collector holds what the seed inferred for `T`"
            );
            then(slot, *session)
        })
        .expect("the cold root runs as a task")
}

const CALLBACK_ROUNDS_FILE: &str = "/semantic-task/callback-rounds.ts";

/// Two generic calls whose callbacks a consumer types. `both` names `f` and
/// then `g`, whose parameter type reads the `U` that typing `f` infers;
/// `echo` names its callback beside a spread argument.
const CALLBACK_ROUNDS: &str = "type Box<T> = { v: T };\n\
     declare function both<T, U, V>(seed: T, f: (x: T) => U, g: (y: U) => V): [T, U, V];\n\
     declare function typedF(x: number): Box<3>;\n\
     declare function typedG(y: Box<3>): string;\n\
     declare function echo<T>(seed: T, f: (x: T) => T): T;\n\
     declare function typedEcho(x: number): number;\n";

fn callback_rounds_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        CALLBACK_ROUNDS_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CALLBACK_ROUNDS),
        crate::FileLanguage::script_ts(),
    );
    host.ensure_indexed_ready(CALLBACK_ROUNDS_FILE)
        .expect("the calls have indexed input");
    host
}

/// The type of the declared function `name`.
fn callback_round_value(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    name: &str,
) -> verter_type_engine::semantic_query::SemanticNodeId {
    let key = dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(
                CALLBACK_ROUNDS_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: verter_type_expr::OwnedStr::from(name),
        },
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    match dispatch.execute_read(key).value() {
        QueryResult::Value(node) => *node,
        other => panic!("`{name}` has a type: {other:?}"),
    }
}

/// Which call a consumer types the callbacks of.
#[derive(Clone, Copy)]
enum RoundCall {
    /// `both(number, f, g)`, `f` typed by `typedF` and `g` by `typedG`: the
    /// candidate's collector holds `T` and `U` when `g`'s parameter type
    /// is mapped.
    TwoCallbacks,
    /// `echo(...[number], f)`, `f` typed by `typedEcho`: no collector is
    /// held, and the typed call is checked again.
    CallbackBesideSpread,
}

/// The call `case` names, the key of the call frame that drives it (its
/// site one past the call's, so the call never meets the frame on the
/// reentry stack), and the argument its consumer types each callback with.
fn round_call(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    case: RoundCall,
) -> (
    verter_type_engine::semantic_query::ResolveCallKey,
    verter_type_engine::semantic_query::ResolveCallKey,
    Vec<verter_type_engine::semantic_query::CallArgKey>,
) {
    use super::call_resolve_tests::eager;
    use verter_type_engine::semantic_query::{
        ArgumentLiteralMode, CallArgKey, CallKind, PayloadArray, SemanticNodeId, TupleElement,
    };
    let argument = |ty: SemanticNodeId, spread: bool, context_sensitive: bool| CallArgKey::Eager {
        first_pass: None,
        ty,
        spread,
        literal_mode: ArgumentLiteralMode::Literal,
        literal_provenance: None,
        context_sensitive,
        const_view: None,
    };
    let number = dispatch
        .graph_arc()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let (callee, args, typed) = match case {
        RoundCall::TwoCallbacks => {
            let f = callback_round_value(dispatch, "typedF");
            let g = callback_round_value(dispatch, "typedG");
            (
                callback_round_value(dispatch, "both"),
                vec![
                    eager(number),
                    argument(f, false, true),
                    argument(g, false, true),
                ],
                vec![eager(number), eager(f), eager(g)],
            )
        }
        RoundCall::CallbackBesideSpread => {
            let f = callback_round_value(dispatch, "typedEcho");
            let tuple = dispatch.graph_arc().intern_node(SemanticNodeData::Tuple {
                elements: PayloadArray::from(
                    vec![TupleElement {
                        label: None,
                        value: number,
                        optional: false,
                        rest: false,
                    }]
                    .into_boxed_slice(),
                ),
                readonly: false,
            });
            (
                callback_round_value(dispatch, "echo"),
                vec![argument(tuple, true, false), argument(f, false, true)],
                vec![argument(tuple, true, false), eager(f)],
            )
        }
    };
    let call = super::call_resolve_tests::call_key_at(
        dispatch,
        CALLBACK_ROUNDS_FILE,
        callee,
        CallKind::Call,
        None,
        args,
    );
    let mut frame = call.clone();
    frame.point.offset += 1;
    (frame, call, typed)
}

/// The physical work call resolutions on `host` did, wherever they ran:
/// the executor walks begun, the candidates whose applicability check
/// began, and the argument relations their first inference pass ran.
fn call_round_work(host: &crate::VerterHost) -> [u64; 3] {
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    [
        stats.call_walks_begun,
        stats.call_candidates_begun,
        stats.call_arguments_related,
    ]
}

fn call_round_work_since(after: [u64; 3], before: [u64; 3]) -> [u64; 3] {
    [
        after[0] - before[0],
        after[1] - before[1],
        after[2] - before[2],
    ]
}

/// The display of the return type a resolved call selected.
fn round_call_answer(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    result: &verter_type_engine::semantic_query::ResolvedCallResult,
) -> String {
    match result {
        verter_type_engine::semantic_query::ResolvedCallResult::Selected {
            return_type, ..
        } => crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
            dispatch,
            *return_type,
            0,
        ),
        other => panic!("the call selects its candidate: {other:?}"),
    }
}

/// `case`'s call driven in place on a fresh host, as a synchronous
/// consumer drives it — each callback typed and the call resumed over it —
/// and the call work that did. Its premise: the round the last callback's
/// typing completes executes the call again, so two walks begin.
fn synchronous_round_call(case: RoundCall) -> (String, [u64; 3]) {
    let (answer, work, _) = synchronous_round_call_with_signatures(case);
    (answer, work)
}

/// [`synchronous_round_call`], with the signature work the call did.
fn synchronous_round_call_with_signatures(
    case: RoundCall,
) -> (
    String,
    [u64; 3],
    super::signature_epoch_tests::SignatureWork,
) {
    let host = callback_rounds_host();
    let _request =
        verter_type_engine::request_context::install_test_request_for(CALLBACK_ROUNDS_FILE);
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let _scope = dispatch.enter_construction_scope();
    let (frame, call, typed) = round_call(&dispatch, case);
    let before = call_round_work(&host);
    let before_signatures = super::signature_epoch_tests::signature_work(&host);
    let step = dispatch.drive_contextual_call_in_place_for_tests(frame, call, typed);
    let work = call_round_work_since(call_round_work(&host), before);
    let signatures = super::signature_epoch_tests::signature_work(&host).since(before_signatures);
    let verter_type_engine::project_semantic_dispatch::call_resolve::ResolveCallStep::Complete(
        result,
    ) = &step
    else {
        panic!("the synchronous consumer's call answers: {step:?}");
    };
    let answer = round_call_answer(&dispatch, result);
    assert_eq!(
        work[0], 2,
        "premise: the call is executed again once its last callback is typed"
    );
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    (answer, work, signatures)
}

/// The reads a call frame needs that are signature lists, once the
/// consumer typed a callback on this thread.
fn signature_lists_after_a_typed_callback(key: &SemanticQueryKey) -> bool {
    matches!(key, SemanticQueryKey::SignaturesOfType { .. })
        && verter_type_engine::project_semantic_dispatch::call_callbacks_typed_here_for_tests() >= 1
}

/// What a call frame driving a contextual call on the continuation runtime
/// did while the reads `held` selects were produced by another task.
struct RoundCallRun {
    /// The first poll, under a probe that forbids every wait, returned
    /// `Pending`.
    yielded: bool,
    /// The callbacks the consumer typed by the end of the first poll.
    typed_at_hold: usize,
    /// The suspensions the call made with a candidate's collector held.
    suspensions: Vec<verter_type_engine::project_semantic_dispatch::CallSuspensionForTests>,
    /// Work done by the first poll.
    at_hold: [u64; 3],
    /// A second poll here while still held, under a probe that forbids
    /// every wait, and then its task polled on another thread, stayed
    /// pending.
    repolled_pending: bool,
    pending_elsewhere: Option<bool>,
    /// Work done by those repolls.
    repoll_work: [u64; 3],
    /// How many reads another task held for the frame.
    held: usize,
    /// The producers going away woke the frame.
    woken: bool,
    /// Work done over the whole polled read.
    work: [u64; 3],
    /// Signature work done over the whole polled read.
    signatures: super::signature_epoch_tests::SignatureWork,
    /// Kernel epochs the read retired as a list was about to be pinned.
    retirements: usize,
    answer: String,
}

/// Resume `entry`'s pending task on another thread until it completes,
/// parking there between polls until woken, and deliver its answer here.
fn resume_round_call_elsewhere(
    entry: &mut verter_type_engine::project_semantic_dispatch::SemanticQueryPoll<
        '_,
        '_,
        crate::resolver_core::HostCapabilities,
    >,
) -> verter_type_engine::semantic_query::CacheRead<QueryResult<SemanticQueryValue>> {
    let task = entry.task().expect("a pending root has a task");
    std::thread::scope(|scope| {
        scope
            .spawn(|| semantic_poll_contract::poll_until_ready(|waker| task.poll(waker)))
            .join()
            .expect("the released task completes on another thread")
    });
    match entry.poll(Waker::noop()) {
        Poll::Ready(read) => read,
        Poll::Pending => panic!("a task completed elsewhere delivers on its entry's thread"),
    }
}

/// `case`'s call driven by a call frame polled at the production entry on a
/// fresh host, its consumer typing each callback as the call names it,
/// while another task produces each read the frame needs that `held`
/// selects; released, the frame's task resumes on another thread and the
/// answer is delivered here.
fn poll_round_call(case: RoundCall, held: Option<fn(&SemanticQueryKey) -> bool>) -> RoundCallRun {
    poll_round_call_retiring(case, held, false)
}

/// [`poll_round_call`]; with `retire`, the signature kernel replaces its
/// epoch once during the first poll, as the first signature list pinned
/// after the consumer typed a callback is about to be pinned.
fn poll_round_call_retiring(
    case: RoundCall,
    held: Option<fn(&SemanticQueryKey) -> bool>,
    retire: bool,
) -> RoundCallRun {
    use verter_type_engine::project_semantic_dispatch::{
        call_callbacks_typed_here_for_tests, drive_contextual_call_here_for_tests,
        hold_call_needs_here_for_tests, take_call_suspensions_for_tests, ContextualCallProbe,
    };
    let host = callback_rounds_host();
    let _request =
        verter_type_engine::request_context::install_test_request_for(CALLBACK_ROUNDS_FILE);
    let mut dispatch = ProjectSemanticDispatch::new(&*host);
    let scope = dispatch.enter_construction_scope();
    let graph = host.project_type_store().semantic_graph();
    let (frame, call, typed) = round_call(&dispatch, case);
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let _ = take_call_suspensions_for_tests();
    let before = call_round_work(&host);
    let before_signatures = super::signature_epoch_tests::signature_work(&host);
    let retirements = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut entry = dispatch.poll_query(SemanticQueryKey::ResolveCall(Box::new(frame)));
    drive_contextual_call_here_for_tests(Some(ContextualCallProbe { call, typed }));
    hold_call_needs_here_for_tests(held);
    let first = {
        let _forbid = verter_execution::blocking::WaitProbe::forbid();
        if retire {
            let (graph, retirements) = (Arc::clone(graph), Arc::clone(&retirements));
            super::signature_epoch_tests::with_build_hook(
                verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint::Pinning,
                move || {
                    if call_callbacks_typed_here_for_tests() >= 1
                        && retirements
                            .compare_exchange(
                                0,
                                1,
                                std::sync::atomic::Ordering::SeqCst,
                                std::sync::atomic::Ordering::SeqCst,
                            )
                            .is_ok()
                    {
                        graph
                            .signature_store()
                            .replace_epoch()
                            .expect("the kernel replaces its epoch");
                    }
                },
                || entry.poll(&waker),
            )
        } else {
            entry.poll(&waker)
        }
    };
    hold_call_needs_here_for_tests(None);
    let typed_at_hold = call_callbacks_typed_here_for_tests();
    drive_contextual_call_here_for_tests(None);
    let at_hold = call_round_work_since(call_round_work(&host), before);
    let suspensions = take_call_suspensions_for_tests();
    let (yielded, held, repolled_pending, pending_elsewhere, repoll_work, read) = match first {
        Poll::Ready(read) => (false, 0, false, None, [0; 3], read),
        Poll::Pending => {
            let repolled_pending = {
                let _forbid = verter_execution::blocking::WaitProbe::forbid();
                entry.poll(&waker).is_pending()
            };
            let pending_elsewhere = entry.task().map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            let repoll_work = call_round_work_since(
                call_round_work_since(call_round_work(&host), before),
                at_hold,
            );
            let held = entry
                .task()
                .map_or(0, |task| task.release_held_call_needs_for_tests());
            let read = resume_round_call_elsewhere(&mut entry);
            (
                true,
                held,
                repolled_pending,
                pending_elsewhere,
                repoll_work,
                read,
            )
        }
    };
    let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
    drop(entry);
    let answer = match read.value() {
        QueryResult::Value(SemanticQueryValue::ResolveCall(result)) => {
            round_call_answer(&dispatch, result.as_ref())
        }
        other => panic!("the frame answers the call it drove: {other:?}"),
    };
    let run = RoundCallRun {
        yielded,
        typed_at_hold,
        suspensions,
        at_hold,
        repolled_pending,
        pending_elsewhere,
        repoll_work,
        held,
        woken,
        work: call_round_work_since(call_round_work(&host), before),
        signatures: super::signature_epoch_tests::signature_work(&host).since(before_signatures),
        retirements: retirements.load(std::sync::atomic::Ordering::SeqCst),
        answer,
    };
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    assert!(graph.resident_flight_keys_for_tests().is_empty());
    drop(scope);
    drop(dispatch);
    run
}

/// A contextual call driven on a frame, whose consumer typed `f`, continues
/// its walk: `f`'s typing relates into the candidate's collector, and
/// mapping `g`'s parameter type under the `U` it inferred reads a
/// signature list another task is producing. The first poll, which may not
/// wait, returns `Pending` at that list with the collector holding `T` and
/// `U` and no candidate check begun. Polled again, here and on another
/// thread, it stays pending and does no work. Released, it resumes on
/// another thread with the walk it began — the synchronous consumer
/// executes the call again — and answers as the synchronous consumer does,
/// with the same candidate and argument work and nothing left open.
#[test]
fn a_contextual_call_waiting_on_a_signature_list_its_resumed_candidate_reads_keeps_its_collector_and_does_not_walk_again(
) {
    let (synchronous, synchronous_work) = synchronous_round_call(RoundCall::TwoCallbacks);
    assert_eq!(
        synchronous_work[1], 1,
        "premise: the synchronous call checks its candidate once, after both callbacks are typed"
    );
    let run = poll_round_call(
        RoundCall::TwoCallbacks,
        Some(signature_lists_after_a_typed_callback),
    );
    assert!(
        run.yielded,
        "the call yields at the signature list instead of waiting the producer out"
    );
    let last = run
        .suspensions
        .last()
        .unwrap_or_else(|| panic!("the call suspends with its collector held"));
    assert!(
        matches!(
            last.needs.as_slice(),
            [SemanticQueryKey::SignaturesOfType { .. }]
        ),
        "it waits on a signature list `g`'s mapping reads: {last:?}"
    );
    assert_eq!(
        last.collected_candidates, 2,
        "its collector holds what the seed inferred for `T` and `f`'s typing for `U`"
    );
    assert_eq!(run.typed_at_hold, 1, "the consumer typed `f`, not yet `g`");
    assert_eq!(
        run.at_hold,
        [1, 0, 0],
        "one walk began, and no candidate check before the list is read"
    );
    assert!(run.held > 0);
    assert!(run.repolled_pending);
    assert_eq!(run.pending_elsewhere, Some(true));
    assert_eq!(run.repoll_work, [0; 3], "a pending repoll does no work");
    assert!(run.woken, "the producer going away wakes the frame");
    assert_eq!(
        run.work,
        [1, synchronous_work[1], synchronous_work[2]],
        "the call walks once across its rounds, with the synchronous candidate and argument work"
    );
    assert_eq!(run.answer, synchronous);
}

/// A contextual call driven on a frame, the signature list `g`'s mapping
/// reads retired after the request delivered it, as the call is about to
/// pin it: the call pins the delivered list in the epoch it was published
/// in, dispatching nothing, and answers as the synchronous consumer does,
/// with the synchronous consumer's result work and nothing left open. Its
/// discovery is the synchronous consumer's plus one walk of the list's
/// three-node subject: a later in-place reader of the same list key finds
/// the retired value refused warm and computes it again in the current
/// epoch.
///
/// Dispatching the retired list again at the pin walks its subject one
/// more time.
#[test]
fn a_contextual_list_retired_before_it_is_pinned_is_read_once() {
    let (synchronous, _, synchronous_signatures) =
        synchronous_round_call_with_signatures(RoundCall::TwoCallbacks);
    let run = poll_round_call_retiring(RoundCall::TwoCallbacks, None, true);
    assert_eq!(
        run.retirements, 1,
        "premise: a list pinned after a typed callback was retired"
    );
    assert!(
        !run.yielded,
        "nothing is held: the frame answers on its first poll"
    );
    assert_eq!(run.answer, synchronous);
    assert_eq!(
        (run.signatures.discovery, run.signatures.forced),
        (
            synchronous_signatures.discovery + 3,
            synchronous_signatures.forced
        ),
        "the retired list is read in its own epoch at the pin; only the later reader's warm refusal walks its subject again"
    );
}

/// A contextual call driven on a frame, nothing held, walks once across
/// both callback rounds — each round continues the walk over its retained
/// plan — where the synchronous consumer's last round executes the call
/// again. It answers as the synchronous consumer does, with the same
/// candidate and argument work.
#[test]
fn a_contextual_call_on_a_frame_walks_once_across_its_callback_rounds_to_the_synchronous_answer() {
    let (synchronous, synchronous_work) = synchronous_round_call(RoundCall::TwoCallbacks);
    let run = poll_round_call(RoundCall::TwoCallbacks, None);
    assert!(
        !run.yielded,
        "nothing is held: the frame answers on its first poll"
    );
    assert_eq!(run.typed_at_hold, 2, "the consumer typed both callbacks");
    assert_eq!(
        run.work,
        [1, synchronous_work[1], synchronous_work[2]],
        "one walk, with the synchronous candidate and argument work"
    );
    assert_eq!(run.answer, synchronous);
}

/// A contextual call waiting, with its collector held, on a signature list
/// its resumed candidate's mapping reads, released while pending — its
/// request abandoned, or the frame's task dropped on another thread —
/// closes what it opened: no session is left that could still collect, no
/// obligation frame stays open, and no flight or wait remains. The call
/// then resolves in place to the synchronous answer.
#[test]
fn a_contextual_call_released_while_its_resumed_candidate_waits_leaves_nothing_open() {
    use verter_type_engine::project_semantic_dispatch::{
        call_callbacks_typed_here_for_tests, drive_contextual_call_here_for_tests,
        hold_call_needs_here_for_tests, take_call_suspensions_for_tests, ContextualCallProbe,
    };
    let (synchronous, _) = synchronous_round_call(RoundCall::TwoCallbacks);
    for dropped_elsewhere in [false, true] {
        let host = callback_rounds_host();
        let _request =
            verter_type_engine::request_context::install_test_request_for(CALLBACK_ROUNDS_FILE);
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (frame, call, typed) = round_call(&dispatch, RoundCall::TwoCallbacks);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let _ = take_call_suspensions_for_tests();
        drive_contextual_call_here_for_tests(Some(ContextualCallProbe {
            call: call.clone(),
            typed: typed.clone(),
        }));
        hold_call_needs_here_for_tests(Some(signature_lists_after_a_typed_callback));
        dispatch
            .with_semantic_task_for_tests(
                SemanticQueryKey::ResolveCall(Box::new(frame.clone())),
                |slot| {
                    let task = slot.as_mut().expect("the task is lent");
                    let pending = {
                        let _forbid = verter_execution::blocking::WaitProbe::forbid();
                        task.poll_for_tests(&waker).is_pending()
                    };
                    hold_call_needs_here_for_tests(None);
                    assert!(pending, "the call yields at the signature list");
                    assert_eq!(call_callbacks_typed_here_for_tests(), 1);
                    let suspensions = take_call_suspensions_for_tests();
                    let last = suspensions
                        .last()
                        .unwrap_or_else(|| panic!("the call suspends with its collector held"));
                    assert!(
                        matches!(
                            last.needs.as_slice(),
                            [SemanticQueryKey::SignaturesOfType { .. }]
                        ),
                        "it waits on a signature list `g`'s mapping reads: {last:?}"
                    );
                    assert_eq!(last.collected_candidates, 2);
                    if dropped_elsewhere {
                        let task = slot.take().expect("the task is lent");
                        std::thread::scope(|scope| {
                            scope
                                .spawn(move || drop(task))
                                .join()
                                .expect("the task drops on another thread");
                        });
                    }
                    // Otherwise the task is left pending: the entry takes it back.
                },
            )
            .expect("the cold root runs as a task");
        drive_contextual_call_here_for_tests(None);
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "the released call closes what it opened (dropped elsewhere: {dropped_elsewhere})"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        let resident = graph.resident_flight_keys_for_tests();
        assert!(
            resident.is_empty(),
            "the call's flights and the held producers go: {resident:?}"
        );
        let step = dispatch.drive_contextual_call_in_place_for_tests(frame, call, typed);
        let verter_type_engine::project_semantic_dispatch::call_resolve::ResolveCallStep::Complete(
            result,
        ) = &step
        else {
            panic!("the call resolves after the release: {step:?}");
        };
        assert_eq!(round_call_answer(&dispatch, result), synchronous);
    }
}

/// A callback named beside a spread argument holds no collector: the typed
/// call is checked again. On a frame the round continues the walk over its
/// retained plan, the typed callback entered into it — the candidate is
/// checked again, as the synchronous consumer's re-execution checks it,
/// but no second walk begins — and answers as the synchronous consumer
/// does.
#[test]
fn a_callback_beside_a_spread_argument_is_rechecked_over_the_retained_plan_without_walking_again() {
    let (synchronous, synchronous_work) = synchronous_round_call(RoundCall::CallbackBesideSpread);
    assert_eq!(
        synchronous_work[1], 2,
        "premise: the typed call's candidate is checked again"
    );
    let run = poll_round_call(RoundCall::CallbackBesideSpread, None);
    assert!(
        !run.yielded,
        "nothing is held: the frame answers on its first poll"
    );
    assert_eq!(run.typed_at_hold, 1, "the consumer typed the callback");
    assert_eq!(run.work[0], 1, "one walk across both rounds");
    assert_eq!(
        run.work[1], synchronous_work[1],
        "the candidate is checked again on the frame too"
    );
    assert_eq!(run.answer, synchronous);
}

/// A contextual call whose callback reads a `typeof` another task is
/// producing suspends its function's evaluation with the call's candidate
/// collector held: `T` holds what `1` inferred, out of the transaction
/// while the task waits. Moved to another thread it stays pending; once the
/// producer goes away it resumes there, the call's held route resumed with
/// that same collector — its executor is not asked again — and answers as
/// an uninterrupted evaluation does, leaving nothing open.
#[test]
fn a_contextual_call_suspends_with_its_collector_held_and_resumes_to_the_uninterrupted_answer() {
    use verter_type_engine::project_semantic_dispatch::ContextualRouteEventForTests as Event;
    let uninterrupted = {
        let host = contextual_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let answer = display_return(&host, &execute_return(&dispatch));
        let events = contextual_routes(&host);
        assert!(
            matches!(
                events.as_slice(),
                [
                    Event::Held { session: held, collected: 1 },
                    Event::Resumed { session: resumed, collected: 1 },
                ] if held == resumed
            ),
            "the callback's cold read is produced by a frame of its own, the call held \
             and resumed with its collector across it: {events:?}"
        );
        answer
    };
    assert_eq!(
        uninterrupted, "[number, Box<3>]",
        "`T` keeps what the seed inferred, `U` is what the callback returns"
    );

    let host = contextual_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let collecting = dispatch.open_request_state_for_tests().collecting_sessions;
    let (answer, held) = suspend_the_contextual_call(&host, &dispatch, &waker, |slot, held| {
        let task = slot.as_mut().expect("the task is lent");
        assert_eq!(
            dispatch.open_request_state_for_tests().collecting_sessions,
            0,
            "the lender holds no collector: the task does"
        );
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    assert!(
                        task.poll_for_tests(&waker).is_pending(),
                        "moved to another thread, the call still waits"
                    );
                })
                .join()
                .expect("the pending poll returns");
        });
        assert!(task.release_held_call_needs_for_tests() > 0);
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the task"
        );
        let answer = std::thread::scope(|scope| {
            scope
                .spawn(|| match task.poll_for_tests(&waker) {
                    Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                    Poll::Ready(TaskOutcomeForTests::Stopped) => panic!("nothing stops the task"),
                    Poll::Pending => panic!("the woken task produces the read itself"),
                })
                .join()
                .expect("the resumed task answers")
        });
        (answer, held)
    });
    assert_eq!(
        contextual_routes(&host),
        vec![Event::Resumed {
            session: held,
            collected: 1
        }],
        "the next run resumes the held route, with the collector it held"
    );
    assert_eq!(display_return(&host, &answer), uninterrupted);
    assert_eq!(
        dispatch.open_request_state_for_tests().collecting_sessions,
        collecting,
        "the answered call leaves the collecting sessions as it found them"
    );
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    assert!(graph.resident_flight_keys_for_tests().is_empty());
}

/// A contextual call suspended with its collector held, then released while
/// pending — its request abandoned, or the task dropped on another thread —
/// releases the collector, its frames' flights and its wait: no session is
/// left that could still collect and nothing stays open. The request goes
/// on to the uninterrupted answer.
#[test]
fn a_contextual_call_released_while_suspended_leaves_nothing_open() {
    use verter_type_engine::project_semantic_dispatch::ContextualRouteEventForTests as Event;
    for dropped_elsewhere in [false, true] {
        let host = contextual_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let collecting = dispatch.open_request_state_for_tests().collecting_sessions;
        let held = suspend_the_contextual_call(&host, &dispatch, &waker, |slot, held| {
            if dropped_elsewhere {
                let task = slot.take().expect("the task is lent");
                std::thread::scope(|scope| {
                    scope
                        .spawn(move || drop(task))
                        .join()
                        .expect("the task drops on another thread");
                });
            }
            held
        });
        assert_eq!(
            contextual_routes(&host),
            vec![Event::Released {
                session: held,
                collected: 1
            }],
            "the held route is released with its collector (dropped elsewhere: {dropped_elsewhere})"
        );
        assert_eq!(
            dispatch.open_request_state_for_tests().collecting_sessions,
            collecting,
            "the released call leaves the collecting sessions as it found them (dropped elsewhere: \
             {dropped_elsewhere})"
        );
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "the released call closes what it opened (dropped elsewhere: {dropped_elsewhere})"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        let resident = graph.resident_flight_keys_for_tests();
        assert!(
            resident.is_empty(),
            "the call's flights, its completed members' and the held producers go: {resident:?}"
        );
        assert_eq!(
            display_return(&host, &execute_return(&dispatch)),
            "[number, Box<3>]"
        );
    }
}

const CALL_VALUE_FILE: &str = "/semantic-task/call-value.ts";

/// Bodies whose calls take the executor route: one suspended at a lowered
/// argument (`f(g(1))`), one suspended at a function-value argument whose
/// body calls again (`run(() => run(() => 1))`), and an overloaded callee
/// whose route runs where it stands (`pick(1)`); and a generic call whose
/// context-sensitive argument is a conditional of function values, which
/// the call names for its consumer to type (`both(1, cond ? .. : ..)`).
const CALL_VALUE: &str = "declare function g(x: number): number;\n\
                          declare function f(x: number): string;\n\
                          declare function run<R>(cb: () => R): R;\n\
                          declare const pick: { (x: number): number; (x: string): string };\n\
                          export function lowered() {\n\
                          return f(g(1));\n\
                          }\n\
                          export function contextual() {\n\
                          return run(() => run(() => 1));\n\
                          }\n\
                          export function overloaded() {\n\
                          return pick(1);\n\
                          }\n\
                          declare function both<T, U>(seed: T, f: (x: T) => U): [T, U];\n\
                          declare const cond: boolean;\n\
                          export function untyped() {\n\
                          return both(1, cond ? (x) => x : (x) => x);\n\
                          }\n";

/// Each root of [`CALL_VALUE`], the calls one evaluation of its body
/// evaluates, and the return it answers.
const CALL_VALUE_ROOTS: [(&str, u64, &str); 3] = [
    ("lowered", 2, "string"),
    ("contextual", 2, "number"),
    ("overloaded", 1, "number"),
];

fn call_value_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        CALL_VALUE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CALL_VALUE),
        crate::FileLanguage::script_ts(),
    );
    host
}

/// The value computations the flow evaluations on `host` began for calls.
fn call_value_runs(host: &crate::VerterHost) -> u64 {
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    stats.call_value_runs
}

/// `name`'s return in [`CALL_VALUE`] read on a fresh host, synchronously or
/// polled: the call value computations the read began, the evaluations of
/// the body it made, and what the read answered and charged.
fn call_value_read(name: &str, polled: bool) -> (u64, u64, semantic_poll_contract::JoinedEvidence) {
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
    let host = call_value_host();
    let mut dispatch = ProjectSemanticDispatch::new(&*host);
    let key = root_return_key(&dispatch, CALL_VALUE_FILE, name);
    let runs = call_value_runs(&host);
    let evaluations = flow_evaluations_for_tests();
    let read = if polled {
        let mut entry = dispatch.poll_query(key);
        let read = semantic_poll_contract::poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        read
    } else {
        dispatch.execute_via_cold_build_helper(key)
    };
    (
        call_value_runs(&host) - runs,
        (flow_evaluations_for_tests() - evaluations) as u64,
        semantic_poll_contract::joined_evidence(&dispatch, &read, |answer| {
            display_return(&host, answer)
        }),
    )
}

/// A call computes its value once each time its body is evaluated, however
/// its executor route ran: suspended at a lowered argument, suspended at a
/// function-value argument, or where it stands. A computation begun again
/// for the same call once its route answered would count again.
///
/// A body evaluated once computes exactly its calls' values. A body its
/// frame evaluates again after a read the frame had to produce first may
/// stop that earlier evaluation before it reaches every call, so there the
/// computations are bounded by the calls of every evaluation.
#[test]
fn a_call_computes_its_value_once() {
    let mut observed = Vec::new();
    for (name, calls, value) in CALL_VALUE_ROOTS {
        for polled in [false, true] {
            let (runs, evaluations, ((answered, ..), _)) = call_value_read(name, polled);
            assert_eq!(
                answered, value,
                "{name} (polled: {polled}) answers its return"
            );
            observed.push((name, polled, calls, runs, evaluations));
        }
    }
    assert!(
        observed.iter().all(|&(_, _, calls, runs, evaluations)| {
            evaluations >= 1 && runs <= calls * evaluations && (evaluations > 1 || runs == calls)
        }),
        "each call computes its value once per evaluation of its body \
         (root, polled, calls, runs, evaluations): {observed:?}"
    );
}

/// A polled read of a body whose calls take the executor route answers
/// what the synchronous read answers: the same return, facts, rails,
/// diagnostics and receipt footprint, and work never below it, for a
/// call suspended at a lowered argument, one suspended at a
/// function-value argument, and one whose route runs where it stands.
#[test]
fn a_polled_call_answers_what_the_synchronous_call_answers() {
    for (name, _, value) in CALL_VALUE_ROOTS {
        let (_, _, synchronous) = call_value_read(name, false);
        let (_, _, polled) = call_value_read(name, true);
        assert_eq!(synchronous.0 .0, value, "{name} answers its return");
        semantic_poll_contract::assert_joined_matches(polled, &synchronous);
    }
}

/// The executor work of `CONTEXTUAL`'s call read synchronously: the walks
/// begun, the candidates whose applicability check began, and the argument
/// relations their first pass ran, its callback round included.
const CONTEXTUAL_ROUND_WORK: [u64; 3] = [0, 0, 0];

/// A contextual call's callback round continues the candidate its first
/// answer named, with the collector the call held: the call's executor work
/// is its first answer's and its round's, never a call begun again over the
/// typed arguments.
#[test]
fn a_contextual_calls_callback_round_continues_its_candidate() {
    let host = contextual_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let before = call_round_work(&host);
    let read = dispatch.execute_via_cold_build_helper(contextual_return_key(&dispatch));
    let work = call_round_work_since(call_round_work(&host), before);
    assert_eq!(display_return(&host, read.value()), "[number, Box<3>]");
    assert_eq!(
        work, CONTEXTUAL_ROUND_WORK,
        "walks begun, candidates begun and arguments related across the callback round"
    );
}

/// A contextual call whose executor names a callback this evaluator does not
/// type (a conditional of function values) settles without it, and leaves no
/// collecting session behind: the call's held collector returns to the
/// transaction abandoned.
#[test]
fn a_contextual_call_whose_callback_is_not_typed_leaves_no_collecting_session() {
    let host = call_value_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let collecting = dispatch.open_request_state_for_tests().collecting_sessions;
    let key = root_return_key(&dispatch, CALL_VALUE_FILE, "untyped");
    let read = dispatch.execute_via_cold_build_helper(key);
    assert!(
        matches!(read.value(), QueryResult::Value(_)),
        "the body answers a return: {:?}",
        read.value()
    );
    assert_eq!(
        dispatch.open_request_state_for_tests().collecting_sessions,
        collecting,
        "the settled call leaves the collecting sessions as it found them"
    );
}

/// The reads a root flow read's own body makes at its own depth are
/// counted, each under the site that made it: the contextual body's read
/// of its callee's signatures (the read its frame joins another task's
/// producer through) is counted, under a driver method.
#[test]
fn each_body_read_of_a_root_flow_read_is_counted_under_its_site() {
    use verter_type_engine::project_semantic_dispatch::flow_return::body_reads;
    use verter_type_engine::semantic_query::SemanticQueryKeyTag;
    const PHASES: [&str; 5] = ["prepare", "prelude", "body", "finish", "close"];
    let host = contextual_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let _ = body_reads::take();
    let _ = dispatch.execute_via_cold_build_helper(contextual_return_key(&dispatch));
    let cold = body_reads::take();
    let signatures: Vec<_> = cold
        .iter()
        .filter(|((_, _, family), _)| *family == SemanticQueryKeyTag::SignaturesOfType)
        .collect();
    assert!(
        !signatures.is_empty(),
        "the body's signature read is counted: {cold:?}"
    );
    assert!(
        signatures
            .iter()
            .all(|((_, site, _), _)| !PHASES.contains(site) && *site != "<unattributed>"),
        "it is counted under the driver method that made it: {signatures:?}"
    );
}

const UTILITY_FILE: &str = "/semantic-task/utility.ts";

/// Bodies whose return argument reads a module constant a closed
/// signature utility types (`ReturnType<() => { v: number }>`, which the
/// checker resolves where it is built): at the root, and in a nested
/// function's return; and a local declared with the same utility, whose
/// annotation is read where no statement suspends.
const UTILITY: &str = "declare const k: ReturnType<() => { v: number }>;\n\
                       export function root() {\n\
                       return k;\n\
                       }\n\
                       export function outer() {\n\
                       return () => {\n\
                       return k;\n\
                       };\n\
                       }\n\
                       export function declared() {\n\
                       let r!: ReturnType<() => { v: number }>;\n\
                       return r;\n\
                       }\n";

fn utility_host() -> Arc<crate::VerterHost> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        UTILITY_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(UTILITY),
        crate::FileLanguage::script_ts(),
    );
    host
}

/// The return of `name` in the utility file, evaluated as a machinery root.
fn utility_return_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    name: &str,
) -> SemanticQueryKey {
    root_return_key(dispatch, UTILITY_FILE, name)
}

/// The return of `name` in `file`, evaluated as a machinery root.
fn root_return_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    file: &str,
    name: &str,
) -> SemanticQueryKey {
    SemanticQueryKey::FlowReturn(Box::new(
        verter_type_engine::semantic_query::FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                verter_type_expr::OwnedStr::from(file),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                verter_type_expr::OwnedStr::from(name),
                verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: verter_type_expr::OwnedSlice::from(Vec::new().into_boxed_slice()),
            context: dispatch.flow_return_context_for(file),
            demand: verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
            input: verter_type_engine::semantic_query::FlowInputContext::empty(),
            result_contract:
                verter_type_engine::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
        },
    ))
}

/// The counted body reads a request answered below the root would leave:
/// a request answered in place, or the request's own driver methods.
fn rows_answered_below_the_root(
    counted: &[(
        verter_type_engine::project_semantic_dispatch::flow_return::body_reads::BodyReadSite,
        u64,
    )],
) -> Vec<String> {
    counted
        .iter()
        .filter(|((_, site, _), _)| {
            matches!(
                *site,
                "read_in_place" | "closed_signature_utility" | "answer_read"
            )
        })
        .map(|row| format!("{row:?}"))
        .collect()
}

/// A root body's return argument a closed signature utility types is read
/// by request: the evaluator requests the utility's instantiation where it
/// stands, and the root read answers it, once. That read is counted under
/// no site: no driver method reads it, and no run answers it in place.
#[test]
fn the_closed_signature_utility_is_read_by_request() {
    use verter_type_engine::project_semantic_dispatch::flow_return::{
        body_reads, flow_read_requests_for_tests,
    };
    let host = utility_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let _ = body_reads::take();
    let requests = flow_read_requests_for_tests();
    let read = dispatch.execute_via_cold_build_helper(utility_return_key(&dispatch, "root"));
    let requested = flow_read_requests_for_tests() - requests;
    let cold = body_reads::take();
    assert_eq!(
        requested, 1,
        "the body requests the utility's read of its root read once: {cold:?}"
    );
    let below = rows_answered_below_the_root(&cold);
    assert!(
        below.is_empty(),
        "no read of the utility is counted: {below:?}"
    );
    let value = display_return(&host, read.value());
    assert!(value.contains("v: number"), "the utility's answer: {value}");
}

/// A nested function's return argument a closed signature utility types
/// is read by request while the nested function's activation is on the
/// root read's stack: the root read answers it, and the nested body
/// resumes with the answer.
#[test]
fn a_nested_functions_closed_signature_utility_is_read_by_request() {
    use verter_type_engine::project_semantic_dispatch::flow_return::{
        body_reads, flow_read_requests_for_tests, nested_body_evaluations_for_tests,
    };
    let host = utility_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let _ = body_reads::take();
    let requests = flow_read_requests_for_tests();
    let bodies = nested_body_evaluations_for_tests();
    let read = dispatch.execute_via_cold_build_helper(utility_return_key(&dispatch, "outer"));
    let requested = flow_read_requests_for_tests() - requests;
    let cold = body_reads::take();
    assert!(
        nested_body_evaluations_for_tests() > bodies,
        "the nested body is evaluated from the stack"
    );
    assert!(
        requested >= 1,
        "the nested body requests the utility's read of the root read: {cold:?}"
    );
    let below = rows_answered_below_the_root(&cold);
    assert!(
        below.is_empty(),
        "no run answers the nested body's request in place: {below:?}"
    );
    let value = display_return(&host, read.value());
    assert!(value.contains("v: number"), "the utility's answer: {value}");
}

/// A declared local's annotation a closed signature utility types is read
/// where no statement suspends: its utility read is answered in place, and
/// it is counted under the in-place answer, by family, so the reads answered
/// in place stay visible.
#[test]
fn a_declared_annotations_closed_signature_utility_is_counted_as_read_in_place() {
    use verter_type_engine::project_semantic_dispatch::flow_return::body_reads;
    use verter_type_engine::semantic_query::SemanticQueryKeyTag;
    let host = utility_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let _ = body_reads::take();
    let _ = dispatch.execute_via_cold_build_helper(utility_return_key(&dispatch, "declared"));
    let cold = body_reads::take();
    assert!(
        cold.iter().any(|((_, site, family), _)| {
            *site == "read_in_place" && *family == SemanticQueryKeyTag::Instantiate
        }),
        "the annotation's utility read is counted as answered in place: {cold:?}"
    );
}

/// A type leaf's read a flow body requests and its root read answers is
/// the read the body would make in place: a root's and a nested function's
/// return, read synchronously and polled, answer the same value with the
/// same receipt, logical work and dependencies either way.
#[test]
fn a_requested_type_leaf_read_answers_and_charges_what_the_in_place_read_does() {
    use verter_type_engine::project_semantic_dispatch::flow_return::{
        flow_read_requests_for_tests, with_type_leaves_read_in_place_for_tests,
    };
    let summary = |name: &str, polled: bool| {
        let host = utility_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let key = utility_return_key(&dispatch, name);
        let requests = flow_read_requests_for_tests();
        let read = if polled {
            let mut entry = dispatch.poll_query(key);
            let read = semantic_poll_contract::poll_until_ready(|waker| entry.poll(waker));
            drop(entry);
            read
        } else {
            dispatch.execute_via_cold_build_helper(key)
        };
        (
            flow_read_requests_for_tests() - requests,
            read_summary(&dispatch, &read),
        )
    };
    for name in ["root", "outer"] {
        for polled in [false, true] {
            let (requested, by_request) = summary(name, polled);
            let (in_place_requests, in_place) =
                with_type_leaves_read_in_place_for_tests(|| summary(name, polled));
            assert!(
                requested >= 1 && in_place_requests == 0,
                "{name} (polled: {polled}) is read by request, then in place: {requested}, {in_place_requests}"
            );
            assert_eq!(by_request, in_place, "{name} (polled: {polled})");
        }
    }
}

/// A receipt's whole structure below its identity, which is an allocation
/// of the host that computed it: its own logical usage and footprint, then
/// each prerequisite it consumed, in order, with how it was consumed and
/// that prerequisite's own structure.
fn receipt_structure(
    receipt: &verter_type_engine::project_semantic_dispatch::cost_receipt::DemandCostReceipt,
) -> String {
    let prerequisites = receipt
        .prerequisites()
        .iter()
        .map(|dependency| {
            format!(
                "{:?} -> {}",
                dependency.nesting,
                receipt_structure(&dependency.receipt)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "{{{:?} {:?} [{}]}}",
        receipt.exclusive(),
        receipt.footprint(),
        prerequisites.join(", ")
    )
}

/// What a read answered, in its own value domain, its fact status, its
/// rails, its diagnostics, and its receipt — the closure's logical work and
/// the whole receipt tree — with the dependencies its value was computed
/// from: the parts of a read a consumer of a root query decides on.
fn read_summary(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    read: &verter_type_engine::semantic_query::CacheRead<QueryResult<SemanticQueryValue>>,
) -> (String, String, bool, bool, Vec<String>, Option<String>) {
    let answer = match read.value() {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => display(
            dispatch,
            &QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
        ),
        QueryResult::Value(SemanticQueryValue::OverloadSet(signatures)) => format!(
            "OverloadSet[{}]",
            signatures
                .iter()
                .map(|signature| format!(
                    "{} {:?}",
                    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                        dispatch,
                        signature.node,
                        0
                    ),
                    signature.occurrence
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    };
    let receipt = read.receipt().priced().map(|receipt| {
        format!(
            "{:?} {} dependencies {:?}",
            receipt.closure_usage(),
            receipt_structure(receipt),
            read.dep_signature(),
        )
    });
    (
        answer,
        format!("{:?}", read.fact_status()),
        read.cache_suppress(),
        read.result_is_partial(),
        read.walker_diagnostics()
            .iter()
            .map(|diagnostic| format!("{diagnostic:?}"))
            .collect(),
        receipt,
    )
}

/// The production polled root entry: a root whose chain reaches a value
/// another task is producing returns `Pending` with its frames suspended,
/// its `Send` task is advanced on other threads through its public poll —
/// pending while the producer holds, ready once it goes away — and the
/// entry then delivers on its own thread exactly what the synchronous
/// entry reads on a fresh host. A warm root answers on its first poll with
/// what the synchronous entry reads.
#[test]
fn a_polled_root_query_suspends_resumes_elsewhere_and_reads_what_the_synchronous_entry_reads() {
    let synchronous = {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let read = dispatch.execute_via_cold_build_helper(typeof_key(&dispatch, "x5"));
        read_summary(&dispatch, &read)
    };
    assert_eq!(synchronous.0, "\"ok\"");

    let host = chain_host();
    let mut dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));

    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let key = typeof_key(&dispatch, "x5");
    let mut entry = dispatch.poll_query(key);
    assert!(entry.poll(&waker).is_pending(), "the root waits on x2");
    {
        let task = entry.task().expect("a pending root has a task");
        assert_send(task);
        assert_eq!(task.live_frames_for_tests(), 3, "x5, x4 and x3 wait on x2");
        std::thread::scope(|scope| {
            scope
                .spawn(|| assert!(task.poll(&waker).is_pending()))
                .join()
                .expect("the pending poll returns");
        });
        drop(lease);
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the task"
        );
        std::thread::scope(|scope| {
            scope
                .spawn(|| assert!(task.poll(&waker).is_ready()))
                .join()
                .expect("the resumed task completes");
        });
    }
    let read = match entry.poll(&waker) {
        Poll::Ready(read) => read,
        Poll::Pending => panic!("a ready task's entry delivers"),
    };
    assert!(entry.task().is_none(), "a delivered root holds no task");
    drop(entry);
    assert_eq!(read_summary(&dispatch, &read), synchronous);
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(other);

    // Warm, the root answers on its first poll, as the synchronous entry
    // answers it.
    let mut warm = dispatch.poll_query(typeof_key(&dispatch, "x5"));
    let Poll::Ready(warm_read) = warm.poll(&waker) else {
        panic!("a warm root answers on its first poll");
    };
    drop(warm);
    let warm_sync = dispatch.execute_via_cold_build_helper(typeof_key(&dispatch, "x5"));
    assert_eq!(
        read_summary(&dispatch, &warm_read),
        read_summary(&dispatch, &warm_sync)
    );
}

/// A polled root released while pending — its consumer abandoned — gives
/// its dispatcher the request state back with nothing left open: its
/// task's frames, flights and wait go, and its operation closes, so the
/// dispatcher answers the same root afterwards as the synchronous entry
/// does.
#[test]
fn a_polled_root_query_released_while_pending_leaves_nothing_open() {
    let host = chain_host();
    let mut dispatch = ProjectSemanticDispatch::new(&*host);
    let graph = host.project_type_store().semantic_graph();
    let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let key = typeof_key(&dispatch, "x5");
    let mut entry = dispatch.poll_query(key);
    assert!(entry.poll(&waker).is_pending());
    assert_eq!(
        graph.resident_flight_keys_for_tests().len(),
        4,
        "the other producer's flight and the root's three"
    );
    drop(entry);
    assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    assert_eq!(
        graph.resident_flight_keys_for_tests().len(),
        1,
        "only the other producer's flight remains"
    );
    drop(lease);
    drop(other);
    let read = dispatch.execute_read(typeof_key(&dispatch, "x5"));
    let QueryResult::Value(node) = read.value() else {
        panic!("the root answers a type: {read:?}");
    };
    assert_eq!(
        display(
            &dispatch,
            &QueryResult::Value(SemanticQueryValue::TypeNode(*node))
        ),
        "\"ok\""
    );
}

/// The contract a consumer that must not block relies on at the production
/// polled entry, for queries off the continuation runtime as for the ones on
/// it: a root whose key another task is producing yields instead of waiting
/// that producer out, keeps its claim across polls, and delivers in its own
/// value domain what the synchronous full-value entry reads.
mod semantic_poll_contract {
    use super::super::signature_epoch_tests::{signature_work, SignatureWork};
    use super::*;

    /// The truthiness classification of `string`: a query off the
    /// continuation runtime answering outside the type-node domain.
    fn classification_key(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> SemanticQueryKey {
        let subject = dispatch
            .graph_arc()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        SemanticQueryKey::ClassifyTruthinessDomain { subject }
    }

    /// How long a held producer stays held when the work run while it is
    /// held does not return: past it the fixture releases the producer
    /// itself, so a poll that waits the producer out still returns and the
    /// test fails on what it observed instead of hanging.
    const HELD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    /// Run `body` while another task holds the producer `lease` on a worker
    /// thread, then release the producer and join the worker: the producer
    /// goes away whether or not `body` returned before the deadline, and what
    /// `body` observed is returned for the caller to assert once everything
    /// it held is released.
    fn while_held<L: Send, R>(lease: L, body: impl FnOnce() -> R) -> R {
        let (release, released) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            let holder = scope.spawn(move || {
                let _ = released.recv_timeout(HELD_DEADLINE);
                drop(lease);
            });
            let observed = body();
            let _ = release.send(());
            holder.join().expect("the holder releases the producer");
            observed
        })
    }

    /// A query off the continuation runtime whose key another task is
    /// producing: the first poll returns `Pending` while the producer holds
    /// instead of waiting it out, the producer going away wakes the entry,
    /// and the next poll — continuing the same claim, which now produces —
    /// delivers in the query's own value domain what the synchronous
    /// full-value entry reads on a fresh host, receipt included.
    #[test]
    fn a_root_off_the_runtime_held_by_another_producer_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = chain_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(classification_key(&dispatch));
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("TruthinessDomain"),
            "the classification answers in its own domain: {synchronous:?}"
        );

        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let key = classification_key(&dispatch);
        let (other, lease) = hold_producer(&host, graph, key.clone());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut entry = dispatch.poll_query(key);
        let first = while_held(lease, || entry.poll(&waker));
        assert!(
            first.is_pending(),
            "the root yields while another task produces its key, instead of waiting it out"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the entry"
        );
        assert!(
            entry.task().is_none(),
            "a root off the runtime lends no task"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken claim produces the key itself"),
        };
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
    }

    /// A query off the continuation runtime released while its claim waits
    /// on another task's producer withdraws its wait and closes what its
    /// entry opened; the producer it waited on is left alone.
    #[test]
    fn a_root_off_the_runtime_released_while_it_waits_leaves_nothing_open() {
        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let key = classification_key(&dispatch);
        let (other, lease) = hold_producer(&host, graph, key.clone());
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let mut entry = dispatch.poll_query(key.clone());
        let (first, waits, released) = while_held(lease, || {
            let first = entry.poll(&waker).is_pending();
            let waits = graph.wait_graph_counts_for_tests().1;
            drop(entry);
            let released = (
                graph.wait_graph_counts_for_tests().1,
                graph.resident_flight_keys_for_tests(),
            );
            (first, waits, released)
        });
        assert!(first, "the root yields while another task produces its key");
        assert_eq!(waits, 1, "the claim waits on the producer");
        assert_eq!(
            released,
            (0, vec![key.clone()]),
            "the released root withdraws its wait and leaves only the other producer's flight"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(key);
        assert!(
            matches!(
                read.value(),
                QueryResult::Value(SemanticQueryValue::TruthinessDomain(_))
            ),
            "the request goes on: {read:?}"
        );
    }

    /// A query off the continuation runtime whose claim waits on another
    /// task's producer is woken by its request's cancellation with that
    /// producer still held, on the thread that polls it — no timer and no
    /// release — and its next poll delivers the terminal cancelled read,
    /// withdrawing its wait while the producer it waited on is left alone.
    #[test]
    fn a_root_off_the_runtime_is_woken_by_its_requests_cancellation_while_the_producer_holds() {
        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let key = classification_key(&dispatch);
        let (other, lease) = hold_producer(&host, graph, key.clone());
        let request = budgeted_request(1);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        let observed = {
            let _request = verter_type_engine::request_context::RequestContextGuard::install(
                Arc::clone(&request),
            );
            let mut entry = dispatch.poll_query(key.clone());
            let observed = while_held(lease, || {
                let first = entry.poll(&waker).is_pending();
                let before = wakes();
                let waits = graph.wait_graph_counts_for_tests().1;
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| request.cancel())
                        .join()
                        .expect("the request is cancelled on another thread");
                });
                let woken = wakes() > before;
                let next = match entry.poll(&waker) {
                    Poll::Ready(read) => Some(read),
                    Poll::Pending => None,
                };
                let left = (
                    graph.wait_graph_counts_for_tests().1,
                    graph.resident_flight_keys_for_tests(),
                );
                (first, before, waits, woken, next, left)
            });
            drop(entry);
            observed
        };
        let (first, before, waits, woken, next, left) = observed;
        assert!(first, "the root yields while another task produces its key");
        assert_eq!(
            before, 0,
            "nothing wakes the root before its request is cancelled"
        );
        assert_eq!(waits, 1, "the claim waits on the producer");
        assert!(
            woken,
            "the request's cancellation wakes the root with the producer still held"
        );
        let read = next.expect("the woken root reads its cancellation without the producer");
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Cancelled)
            ),
            "the cancelled root reads a terminal cancellation: {read:?}"
        );
        assert_eq!(
            left,
            (0, vec![key.clone()]),
            "the cancelled root withdraws its wait and leaves only the other producer's flight"
        );
        assert_eq!(
            Arc::strong_count(&counter),
            2,
            "a terminal root retains neither readiness nor cancellation wakers"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(key);
        assert!(
            matches!(
                read.value(),
                QueryResult::Value(SemanticQueryValue::TruthinessDomain(_))
            ),
            "the cancellation published nothing reusable: {read:?}"
        );
    }

    /// Roots answering outside the type-node domain poll their own value:
    /// a machinery-root flow return on the continuation runtime and a
    /// classification off it deliver what the synchronous full-value entry
    /// reads, never a domain mismatch; the node adapter is the one place a
    /// consumer narrows.
    #[test]
    fn a_root_answering_outside_the_node_domain_polls_its_own_value() {
        let waker = Waker::from(Arc::new(CountingWaker::default()));

        let synchronous = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            display_return(&host, &execute_return(&dispatch))
        };
        let host = contextual_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let mut entry = dispatch.poll_query(contextual_return_key(&dispatch));
        let Poll::Ready(read) = entry.poll(&waker) else {
            panic!("nothing holds the flow return's reads");
        };
        drop(entry);
        assert_eq!(display_return(&host, read.value()), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);

        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let synchronous = dispatch.execute_via_cold_build_helper(classification_key(&dispatch));
        let synchronous = read_summary(&dispatch, &synchronous);
        let mut entry = dispatch.poll_query(classification_key(&dispatch));
        let Poll::Ready(read) = entry.poll(&waker) else {
            panic!("a warm classification answers on its first poll");
        };
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        let mut entry = dispatch.poll_query(classification_key(&dispatch));
        let Poll::Ready(node) = entry.poll_node(&waker) else {
            panic!("a warm classification answers on its first poll");
        };
        drop(entry);
        assert!(
            matches!(
                node.value(),
                QueryResult::Error(
                    verter_type_engine::semantic_query::QueryError::ValueDomainMismatch { .. }
                )
            ),
            "the node adapter narrows as the synchronous node entry does: {node:?}"
        );
    }

    /// The single-threaded control: one thread polls a root whose chain
    /// waits on another task's producer, with no other thread advancing it,
    /// and reads what the synchronous entry reads once the producer goes
    /// away. With optional capture compiled in (`semantic-observe`) the same
    /// control runs unchanged.
    #[test]
    fn a_root_polled_on_one_thread_suspends_and_resumes_to_the_synchronous_read() {
        let synchronous = {
            let host = chain_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(typeof_key(&dispatch, "x5"));
            read_summary(&dispatch, &read)
        };
        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (other, lease) = hold_producer(&host, graph, typeof_key(&dispatch, "x2"));
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut entry = dispatch.poll_query(typeof_key(&dispatch, "x5"));
        let first = while_held(lease, || entry.poll(&waker));
        assert!(first.is_pending(), "the chain waits on x2");
        assert!(counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken root produces x2 itself"),
        };
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
    }

    pub(super) type JoinedEvidence = (
        (String, String, bool, bool, Vec<String>, Option<String>),
        Option<u64>,
    );

    /// What a root that joined another task's producer through its frame
    /// reads, beside the work its receipt records: everything
    /// [`read_summary`] compares, the receipt's footprint, depth and
    /// prerequisites in place of its work, with `answer` rendering the value.
    /// The frame's run that met the producer is run again once the producer
    /// answers, and each entry that run makes again is charged again, so the
    /// work is compared on its own: never below the synchronous entry's.
    /// (Exact equality is asserted by the body-once control until the flow
    /// frame retains its evaluation.)
    pub(super) fn joined_evidence(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        read: &verter_type_engine::semantic_query::CacheRead<QueryResult<SemanticQueryValue>>,
        answer: impl FnOnce(&QueryResult<SemanticQueryValue>) -> String,
    ) -> JoinedEvidence {
        let (_, facts, suppress, partial, diagnostics, _) = read_summary(dispatch, read);
        let receipt = read.receipt().priced();
        (
            (
                answer(read.value()),
                facts,
                suppress,
                partial,
                diagnostics,
                receipt.map(|receipt| {
                    format!(
                        "{:?}",
                        (
                            receipt.footprint(),
                            receipt.depth(),
                            receipt.prerequisites().len()
                        )
                    )
                }),
            ),
            receipt.map(|receipt| receipt.exclusive().work),
        )
    }

    /// Assert a joined read against its synchronous read
    /// ([`joined_evidence`]).
    pub(super) fn assert_joined_matches(joined: JoinedEvidence, synchronous: &JoinedEvidence) {
        assert_eq!(joined.0, synchronous.0);
        let (Some(joined), Some(synchronous)) = (joined.1, synchronous.1) else {
            panic!("both complete reads carry a receipt");
        };
        assert!(
            joined >= synchronous,
            "a joined read is charged every entry its synchronous read is: {joined} < {synchronous}"
        );
    }

    /// `boolean extends true ? string : number`, distributed.
    fn distributed_union_key(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> SemanticQueryKey {
        let graph = dispatch.graph_arc();
        let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
        let yes = graph.intern_node(SemanticNodeData::Literal(
            verter_type_engine::semantic_query::LiteralValue::Boolean(true),
        ));
        let SemanticQueryKey::Conditional {
            true_branch,
            false_branch,
            ..
        } = primitive_conditional_key(dispatch, boolean, yes)
        else {
            unreachable!("the key is a conditional");
        };
        SemanticQueryKey::Conditional {
            check: boolean,
            extends: yes,
            true_branch,
            false_branch,
            distributive: true,
            pending: None,
        }
    }

    /// The final reduction of a distributed decision. Hold both member
    /// orders so the test reaches the producer independently of ordering.
    fn member_unions(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> [SemanticQueryKey; 2] {
        let graph = dispatch.graph_arc();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        [[string, number], [number, string]].map(|members| SemanticQueryKey::ReduceUnion {
            members: Arc::from(members),
            nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
        })
    }

    #[test]
    fn distributed_reductions_share_one_canonical_union_candidate() {
        let host = chain_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _builder = dispatch.enter_construction_scope();
        let key = distributed_union_key(&dispatch);
        let SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } = key.clone()
        else {
            unreachable!("the distributed key is a conditional");
        };
        let swapped = SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch: false_branch,
            false_branch: true_branch,
            distributive,
            pending,
        };
        for key in [key, swapped] {
            let read = dispatch.execute_via_cold_build_helper(key);
            assert_eq!(display(&dispatch, read.value()), "Union(number | string)");
            assert!(!read.result_is_partial());
        }
        let graph = host.project_type_store().semantic_graph();
        assert_eq!(
            member_unions(&dispatch)
                .iter()
                .filter(|key| graph.contains_key(key))
                .count(),
            1,
            "member order must not create another reduction candidate"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    }

    /// A distribution's final reduction, which another task is producing,
    /// makes the decision's frame return `Pending`
    /// instead of waiting that producer out on the polling thread. The
    /// producer going away wakes the root; the frame then produces the
    /// query itself, and the root delivers exactly what the synchronous
    /// entry reads on a fresh host — answer, completeness, diagnostics and
    /// receipt — with nothing left open.
    #[test]
    fn a_decision_joining_another_tasks_reduction_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = chain_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let _builder = dispatch.enter_construction_scope();
            let read = dispatch.execute_via_cold_build_helper(distributed_union_key(&dispatch));
            read_summary(&dispatch, &read)
        };
        assert_eq!(synchronous.0, "Union(number | string)");
        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _builder = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let [first_order, second_order] = member_unions(&dispatch);
        let (first_task, first_lease) = hold_producer(&host, graph, first_order);
        let (second_task, second_lease) = hold_producer(&host, graph, second_order);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let key = distributed_union_key(&dispatch);
        let members = || {
            ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::conditional_distributed_members_for_tests()
        };
        let before = members();
        let mut entry = dispatch.poll_query(key.clone());
        let (first, again, before_release) = while_held((first_lease, second_lease), || {
            let first = entry.poll(&waker).is_pending();
            let completed = members() - before;
            let again = first && entry.poll(&waker).is_pending();
            (first, again, completed)
        });
        assert!(
            first && again,
            "the decision yields while another task produces its union, instead of waiting it out"
        );
        assert_eq!(
            before_release, 2,
            "both distribution members already completed"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken decision produces the union itself"),
        };
        drop(entry);
        assert_eq!(
            members() - before,
            2,
            "completed members are never folded again"
        );
        let joined = read_summary(&dispatch, &read);
        assert_eq!(
            joined, synchronous,
            "a suspended reduction consumes its delivery without repeating completed work"
        );
        let warm = dispatch.execute_via_cold_build_helper(key);
        assert_eq!(read_summary(&dispatch, &warm), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop((first_task, second_task));
    }

    /// The callable-shape reads of `both`'s type a flow evaluation of `f`
    /// makes itself: a query no frame family owns.
    fn callee_signature_reads(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> Vec<SemanticQueryKey> {
        [ProjectionMode::Expanded, ProjectionMode::Navigate]
            .into_iter()
            .filter_map(|mode| {
                let callee = dispatch.execute_read(dispatch.typeof_key_for(
                    ValueRootKey {
                        scope: ScopeId::file(
                            CONTEXTUAL_FILE,
                            verter_type_expr::TopLevelOwnerId::ordinary_file(),
                        ),
                        name: "both".into(),
                    },
                    ProjectionReductionContext::published(mode),
                ));
                match callee.value() {
                    QueryResult::Value(callee) => Some(SemanticQueryKey::SignaturesOfType {
                        subject: *callee,
                        kind: verter_type_engine::semantic_query::SignatureKind::Call,
                        context: verter_type_engine::semantic_query::SemanticContextId::production(
                        ),
                    }),
                    _ => None,
                }
            })
            .collect()
    }

    /// A flow evaluation's own read of a query no frame family owns, which
    /// another task is producing, makes the flow frame return `Pending`
    /// instead of waiting that producer out on the polling thread, and the
    /// woken root answers what the synchronous entry answers on a fresh host.
    #[test]
    fn a_flow_joining_another_tasks_signature_read_yields_and_answers_what_the_synchronous_entry_answers(
    ) {
        let synchronous = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(contextual_return_key(&dispatch));
            joined_evidence(&dispatch, &read, |answer| display_return(&host, answer))
        };
        assert!(!synchronous.0 .3, "the return is complete: {synchronous:?}");
        let host = contextual_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        // The evaluation checks the callee at either site; another task
        // produces both.
        let mut keys = callee_signature_reads(&dispatch);
        keys.dedup();
        let (tasks, leases): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .flat_map(|key| {
                [
                    hold_producer(&host, graph, key.clone()),
                    hold_producer_with(&host, graph, key, ReadCapture::expression_for_tests()),
                ]
            })
            .unzip();
        assert!(!leases.is_empty(), "the callee has a callable type");
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut entry = dispatch.poll_query(contextual_return_key(&dispatch));
        let first = while_held(leases, || entry.poll(&waker));
        assert!(
            first.is_pending(),
            "the flow yields while another task produces its callee's signatures"
        );
        assert!(counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken flow reads the signatures itself"),
        };
        drop(entry);
        assert_joined_matches(
            joined_evidence(&dispatch, &read, |answer| display_return(&host, answer)),
            &synchronous,
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(tasks);
    }

    /// A flow frame whose own body's read another task is producing keeps
    /// its evaluation across the suspension: once the producer goes away
    /// the body is evaluated once in all, and the root delivers exactly the
    /// synchronous entry's evidence, logical work included.
    #[test]
    #[ignore = "a flow frame still drops its run at an unanswered read and evaluates the body again after the delivery (5 evaluations and 71 work against 4 and 68): the frame does not yet keep its evaluator across the read"]
    fn a_flow_suspended_at_a_held_read_evaluates_its_body_once() {
        use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
        let (synchronous, synchronous_evaluations) = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let before = flow_evaluations_for_tests();
            let read = dispatch.execute_via_cold_build_helper(contextual_return_key(&dispatch));
            (
                joined_evidence(&dispatch, &read, |answer| display_return(&host, answer)),
                flow_evaluations_for_tests() - before,
            )
        };
        let host = contextual_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let mut keys = callee_signature_reads(&dispatch);
        keys.dedup();
        let (tasks, leases): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .flat_map(|key| {
                [
                    hold_producer(&host, graph, key.clone()),
                    hold_producer_with(&host, graph, key, ReadCapture::expression_for_tests()),
                ]
            })
            .unzip();
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let before = flow_evaluations_for_tests();
        let mut entry = dispatch.poll_query(contextual_return_key(&dispatch));
        let first = while_held(leases, || entry.poll(&waker));
        assert!(first.is_pending(), "the flow yields while its read is held");
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken flow reads the signatures itself"),
        };
        drop(entry);
        assert_eq!(
            (
                joined_evidence(&dispatch, &read, |answer| display_return(&host, answer)),
                flow_evaluations_for_tests() - before,
            ),
            (synchronous, synchronous_evaluations),
            "the suspended flow evaluates its body once and reads exactly what the synchronous entry reads"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        drop(tasks);
    }

    /// Hold every producer of the callee signature reads `f`'s body makes,
    /// in both capture modes.
    fn hold_callee_signatures<'g>(
        host: &crate::VerterHost,
        graph: &'g verter_type_engine::semantic_query_memo::SemanticGraphStore,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (
        Vec<verter_execution::tasks::ExecutionTask>,
        Vec<verter_type_engine::semantic_query_memo::ProducerLease<'g>>,
    ) {
        let mut keys = callee_signature_reads(dispatch);
        keys.dedup();
        keys.into_iter()
            .flat_map(|key| {
                [
                    hold_producer(host, graph, key.clone()),
                    hold_producer_with(host, graph, key, ReadCapture::expression_for_tests()),
                ]
            })
            .unzip()
    }

    /// A flow frame held at its body's own read is owned `Send` state: its
    /// task stays pending on another thread while the producer holds, and
    /// once the producer goes away it is advanced to readiness on that
    /// thread, the body continuing where it stopped. The entry then delivers
    /// exactly the synchronous entry's evidence, logical work included, the
    /// body evaluated once across both threads.
    #[test]
    #[ignore = "a flow frame still drops its run at an unanswered read and evaluates the body again after the delivery: the frame does not yet keep its evaluator across the read"]
    fn a_flow_held_at_its_bodys_read_resumes_on_another_thread_to_the_synchronous_read() {
        use verter_type_engine::project_semantic_dispatch::flow_return::flow_evaluations_for_tests;
        let (synchronous, synchronous_evaluations) = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let before = flow_evaluations_for_tests();
            let read = dispatch.execute_via_cold_build_helper(contextual_return_key(&dispatch));
            (
                joined_evidence(&dispatch, &read, |answer| display_return(&host, answer)),
                flow_evaluations_for_tests() - before,
            )
        };
        let host = contextual_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (tasks, leases) = hold_callee_signatures(&host, graph, &dispatch);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = flow_evaluations_for_tests();
        let mut entry = dispatch.poll_query(contextual_return_key(&dispatch));
        let (first, elsewhere) = while_held(leases, || {
            let first = entry.poll(&waker).is_pending();
            let elsewhere = entry.task().map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| {
                            let before = flow_evaluations_for_tests();
                            let pending = task.poll(&waker).is_pending();
                            (pending, flow_evaluations_for_tests() - before)
                        })
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, elsewhere)
        });
        assert!(first, "the flow yields while its body's read is held");
        let Some((still_pending, held_evaluations)) = elsewhere else {
            panic!("the held flow runs as a task");
        };
        assert!(
            still_pending,
            "its task stays pending on another thread while the producer holds"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let (ready, resumed_evaluations) = {
            let task = entry.task().expect("the woken flow still runs as a task");
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let before = flow_evaluations_for_tests();
                        let ready = task.poll(&waker).is_ready();
                        (ready, flow_evaluations_for_tests() - before)
                    })
                    .join()
                    .expect("the resumed poll returns")
            })
        };
        assert!(ready, "the woken flow completes on the other thread");
        let here = flow_evaluations_for_tests() - before;
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the entry delivers what its task completed"),
        };
        drop(entry);
        assert_eq!(
            (
                joined_evidence(&dispatch, &read, |answer| display_return(&host, answer)),
                here + held_evaluations + resumed_evaluations,
            ),
            (synchronous, synchronous_evaluations),
            "the flow resumed elsewhere evaluates its body once and reads exactly what the synchronous entry reads"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(tasks);
    }

    /// A flow frame held at its body's own read and released — dropped by
    /// its consumer, or stopped by its request's cancellation, which wakes it
    /// with the producer still held — drops the evaluation it retained: no
    /// contextual call stays held, nothing is left open but the holders'
    /// flights, and the dispatcher then reads the flow as the synchronous
    /// entry does.
    #[test]
    fn a_flow_released_or_cancelled_while_its_bodys_read_is_held_leaves_nothing_open() {
        use verter_type_engine::project_semantic_dispatch::ContextualRouteEventForTests as Event;
        // What a read answers, apart from its receipt: a read after the
        // release finds what the released flow already produced warm.
        let answered =
            |dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
             host: &crate::VerterHost| {
                let read = dispatch.execute_via_cold_build_helper(contextual_return_key(dispatch));
                let ((answer, facts, suppress, partial, diagnostics, _), _) =
                    joined_evidence(dispatch, &read, |answer| display_return(host, answer));
                (answer, facts, suppress, partial, diagnostics)
            };
        let synchronous = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            answered(&dispatch, &host)
        };
        for cancelled in [false, true] {
            let host = contextual_host();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let graph = host.project_type_store().semantic_graph();
            let (tasks, leases) = hold_callee_signatures(&host, graph, &dispatch);
            let holders = graph.resident_flight_keys_for_tests().len();
            let counter = Arc::new(CountingWaker::default());
            let waker = Waker::from(Arc::clone(&counter));
            let _ = contextual_routes(&host);
            let (pending, stopped, flights) = while_held(leases, || {
                if cancelled {
                    let request = budgeted_request(1);
                    let _request =
                        verter_type_engine::request_context::RequestContextGuard::install(
                            Arc::clone(&request),
                        );
                    dispatch
                        .with_semantic_task_for_tests(contextual_return_key(&dispatch), |slot| {
                            let task = slot.as_mut().expect("the task is lent");
                            let pending = task.poll_for_tests(&waker).is_pending();
                            let before = counter.0.load(std::sync::atomic::Ordering::SeqCst);
                            request.cancel();
                            let woken =
                                counter.0.load(std::sync::atomic::Ordering::SeqCst) > before;
                            let stopped = woken
                                && matches!(
                                    task.poll_for_tests(&waker),
                                    Poll::Ready(TaskOutcomeForTests::Stopped)
                                )
                                && task.live_frames_for_tests() == 0;
                            (
                                pending,
                                stopped,
                                graph.resident_flight_keys_for_tests().len(),
                            )
                        })
                        .expect("the flow root runs as a task")
                } else {
                    let mut entry = dispatch.poll_query(contextual_return_key(&dispatch));
                    let pending = entry.poll(&waker).is_pending();
                    drop(entry);
                    (pending, true, graph.resident_flight_keys_for_tests().len())
                }
            });
            let label = format!("cancelled={cancelled}");
            assert!(pending, "{label}: the flow yields at its body's read");
            assert!(
                stopped,
                "{label}: the cancellation wakes the waiting flow, which stops"
            );
            assert_eq!(
                flights, holders,
                "{label}: only the holders' flights of the held reads remain"
            );
            let routes = contextual_routes(&host);
            let held_routes = routes
                .iter()
                .filter(|event| matches!(event, Event::Held { .. }))
                .count();
            let ended_routes = routes
                .iter()
                .filter(|event| matches!(event, Event::Released { .. } | Event::Resumed { .. }))
                .count();
            assert_eq!(
                held_routes, ended_routes,
                "{label}: every contextual call the flow held is released: {routes:?}"
            );
            assert_eq!(
                dispatch.open_request_state_for_tests(),
                NOTHING_OPEN,
                "{label}"
            );
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "{label}");
            drop(tasks);
            assert_eq!(answered(&dispatch, &host), synchronous, "{label}");
        }
    }

    /// The overload set of `both`'s call signatures — a family built in one
    /// step — and the signature-list reads its build makes.
    fn overload_set_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let reads = callee_signature_reads(dispatch);
        let SemanticQueryKey::SignaturesOfType { subject, .. } = reads[0] else {
            unreachable!("the reads are signature-list reads")
        };
        let set = SemanticQueryKey::ResolveOverloadSet {
            callee: subject,
            kind: verter_type_engine::semantic_query::SignatureKind::Call,
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: verter_type_engine::semantic_query::OverloadSetContext {
                resolve_env_hash: host
                    .host_view_env_hashes_for(CONTEXTUAL_FILE)
                    .resolve_env_hash,
                ..Default::default()
            },
        };
        (set, reads)
    }

    /// An overload set whose callee's signature list another task is
    /// producing returns `Pending` at the polled entry instead of waiting
    /// that producer out inside the poll; its `Send` task stays pending on
    /// another thread while the producer holds. Once the producer goes away
    /// the set resumes from its retained first stage, running no stage
    /// again, and reads exactly what the synchronous entry — which drives
    /// the same continuation in place — reads on a fresh host: answer, fact
    /// status, diagnostics and receipt, logical work included. Warm, the
    /// polled and synchronous entries answer alike without running a stage.
    #[test]
    fn an_overload_set_whose_signature_read_is_held_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = contextual_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (set, _) = overload_set_keys(&host, &dispatch);
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(set);
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (1, 1),
                "the set begins once and reads its callee's list once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("OverloadSet"),
            "the set answers in its own domain: {synchronous:?}"
        );
        let host = contextual_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (set, mut reads) = overload_set_keys(&host, &dispatch);
        reads.dedup();
        let (tasks, leases): (Vec<_>, Vec<_>) = reads
            .into_iter()
            .flat_map(|key| {
                [
                    hold_producer(&host, graph, key.clone()),
                    hold_producer_with(&host, graph, key, ReadCapture::expression_for_tests()),
                ]
            })
            .unzip();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(set.clone());
        let (first, frames, elsewhere) = while_held(leases, || {
            let first = entry.poll(&waker).is_pending();
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, elsewhere)
        });
        assert!(
            first,
            "the overload set yields while its signature read is held"
        );
        assert_eq!(frames, Some(1), "the set's own frame waits");
        assert_eq!(
            elsewhere,
            Some(true),
            "its task stays pending on another thread while the producer holds"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken overload set reads the signatures itself"),
        };
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (1, 1),
            "no stage runs again: the list is read once"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);

        let mut warm = dispatch.poll_query(set.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm overload set answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(set);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            after,
            "a warm overload set runs no stage"
        );
        drop(tasks);
    }

    /// A callee whose call signatures differ by arm: its shared list
    /// synthesizes one composite candidate, whose node form reads the
    /// result its call map applies.
    const UNION_CALLEE: &str = "declare const pick: ((a: string) => 1) | ((a: string) => 2);\n";

    fn union_callee_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            CONTEXTUAL_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(UNION_CALLEE),
            crate::FileLanguage::script_ts(),
        );
        host
    }

    /// The overload set of `pick`'s call signatures, and its list read.
    fn union_overload_set_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let callee = dispatch.execute_read(dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    CONTEXTUAL_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: "pick".into(),
            },
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ));
        let QueryResult::Value(callee) = callee.value() else {
            panic!("`pick` has a type: {callee:?}");
        };
        let kind = verter_type_engine::semantic_query::SignatureKind::Call;
        let set = SemanticQueryKey::ResolveOverloadSet {
            callee: *callee,
            kind,
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: verter_type_engine::semantic_query::OverloadSetContext {
                resolve_env_hash: host
                    .host_view_env_hashes_for(CONTEXTUAL_FILE)
                    .resolve_env_hash,
                ..Default::default()
            },
        };
        let list = SemanticQueryKey::SignaturesOfType {
            subject: *callee,
            kind,
            context: verter_type_engine::semantic_query::SemanticContextId::production(),
        };
        (set, list)
    }

    /// An overload set over a composite candidate, its list held by another
    /// task: the poll yields, and once released the set settles the
    /// composite from its retained list — reading the composite's result as
    /// its next stage, the list never again — and reads what the
    /// synchronous entry reads. Released while pending instead, the set
    /// leaves nothing open and the dispatcher answers it afterwards as the
    /// synchronous entry does.
    #[test]
    fn an_overload_set_over_a_composite_candidate_resumes_at_its_composite_and_releases_cleanly() {
        let synchronous = {
            let host = union_callee_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (set, _) = union_overload_set_keys(&host, &dispatch);
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(set);
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (1, 2),
                "the set begins once, reads its list, then its composite's result"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("OverloadSet") && !synchronous.3,
            "the set over the composite is complete: {synchronous:?}"
        );

        let host = union_callee_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (set, list) = union_overload_set_keys(&host, &dispatch);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let before = continuation_stages(&host);
        let (other, lease) = hold_producer(&host, graph, list.clone());
        let mut entry = dispatch.poll_query(set.clone());
        let first = while_held(lease, || entry.poll(&waker));
        assert!(first.is_pending(), "the set yields while its list is held");
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken set reads its list itself"),
        };
        drop(entry);
        drop(other);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (1, 2),
            "no stage runs again: the list and the composite's result are each read once"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);

        let host = union_callee_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (set, list) = union_overload_set_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, list);
        let mut entry = dispatch.poll_query(set.clone());
        assert!(entry.poll(&waker).is_pending());
        drop(entry);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert_eq!(
            graph.resident_flight_keys_for_tests().len(),
            1,
            "only the other producer's flight remains"
        );
        drop(lease);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(set);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// What a read decides on, receipt aside: its answer, fact status,
    /// rails and diagnostics.
    type ReadDecision = (String, String, bool, bool, Vec<String>);

    fn decision(
        summary: (String, String, bool, bool, Vec<String>, Option<String>),
    ) -> ReadDecision {
        (summary.0, summary.1, summary.2, summary.3, summary.4)
    }

    /// What a read decided and the work it did, polled across one kernel
    /// epoch replacement, beside the synchronous entry's read with none.
    struct RetiredRead {
        polled: ReadDecision,
        synchronous: ReadDecision,
        /// Builds begun and stages resumed by the polled read.
        stages: (u64, u64),
        work: SignatureWork,
        synchronous_work: SignatureWork,
    }

    /// `key` polled at the production entry on a fresh `host` while the
    /// signature kernel replaces its epoch exactly once, at the first time a
    /// build or a list reaches `point` — as a list's build settles the list
    /// it publishes, as a consumer about to pin the completed list holds it,
    /// or as a composite candidate's result read has its result — against
    /// what the synchronous entry reads on a fresh host with no replacement.
    fn read_across_one_retirement(
        point: verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint,
        host: fn() -> Arc<crate::VerterHost>,
        key: fn(
            &crate::VerterHost,
            &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        ) -> SemanticQueryKey,
    ) -> RetiredRead {
        let (synchronous, synchronous_work) = {
            let host = host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let key = key(&host, &dispatch);
            let before = signature_work(&host);
            let read = dispatch.execute_via_cold_build_helper(key);
            let work = signature_work(&host).since(before);
            (decision(read_summary(&dispatch, &read)), work)
        };
        let host = host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let key = key(&host, &dispatch);
        let graph = Arc::clone(dispatch.graph_arc());
        let replacements = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let before = continuation_stages(&host);
        let before_work = signature_work(&host);
        let read = {
            let (graph, replacements) = (Arc::clone(&graph), Arc::clone(&replacements));
            super::super::signature_epoch_tests::with_build_hook(
                point,
                move || {
                    if replacements
                        .compare_exchange(
                            0,
                            1,
                            std::sync::atomic::Ordering::SeqCst,
                            std::sync::atomic::Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        graph
                            .signature_store()
                            .replace_epoch()
                            .expect("the kernel replaces its epoch");
                    }
                },
                || {
                    let mut entry = dispatch.poll_query(key);
                    poll_until_ready(|waker| entry.poll(waker))
                },
            )
        };
        assert_eq!(
            replacements.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "premise: exactly one epoch is retired at {point:?}"
        );
        let after = continuation_stages(&host);
        let work = signature_work(&host).since(before_work);
        let polled = decision(read_summary(&dispatch, &read));
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        RetiredRead {
            polled,
            synchronous,
            stages: (after.0 - before.0, after.1 - before.1),
            work,
            synchronous_work,
        }
    }

    /// A signature list the kernel retired before its consumer read it —
    /// as its build settled it, or after the request completed it, as its
    /// consumer held it to pin — is read through the epoch it was published
    /// in: the consumer pins the delivered list and never reads it again.
    /// An overload set over authored leaves reads its list once; one over a
    /// composite candidate reads its list once and names the composite's
    /// result after one pass copies the composite into the current epoch;
    /// an apparent type reads its anchor list once. Each reads what the
    /// synchronous entry reads with no replacement, with the synchronous
    /// entry's discovery, matching and result work.
    ///
    /// Reading the retired list once more instead runs one stage more per
    /// read, and its overload sets walk their callee again.
    #[test]
    fn a_signature_list_retired_before_its_consumer_reads_it_is_read_through_its_own_epoch() {
        use verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint;
        let shape = |work: SignatureWork| (work.discovery, work.positions, work.forced);
        for point in [
            SignaturesOfTypeBuildPoint::Settled,
            SignaturesOfTypeBuildPoint::Pinning,
        ] {
            let overloads = read_across_one_retirement(point, contextual_host, |host, dispatch| {
                overload_set_keys(host, dispatch).0
            });
            assert!(
                overloads.synchronous.0.contains("OverloadSet"),
                "premise: {:?}",
                overloads.synchronous
            );
            assert_eq!(
                overloads.polled, overloads.synchronous,
                "{point:?}: an overload set over authored leaves"
            );
            assert_eq!(
                overloads.stages,
                (1, 1),
                "{point:?}: the set reads its list once"
            );
            assert_eq!(
                (overloads.work.rebinds, overloads.work.descriptors),
                (0, 0),
                "{point:?}: authored leaves are graph nodes once pinned: nothing is copied"
            );
            assert_eq!(
                shape(overloads.work),
                shape(overloads.synchronous_work),
                "{point:?}: the callee is walked once"
            );

            let composite =
                read_across_one_retirement(point, union_callee_host, |host, dispatch| {
                    union_overload_set_keys(host, dispatch).0
                });
            assert!(
                composite.synchronous.0.contains("OverloadSet") && !composite.synchronous.3,
                "premise: {:?}",
                composite.synchronous
            );
            // `pick`'s union and its two arms are walked, the arms' one
            // parameter position is compared once each way, and the
            // composite's result is read once.
            assert_eq!(
                shape(composite.synchronous_work),
                (3, 2, 1),
                "premise: the synchronous read's work"
            );
            assert_eq!(
                composite.polled, composite.synchronous,
                "{point:?}: an overload set over a composite"
            );
            assert_eq!(
                composite.stages,
                (1, 2),
                "{point:?}: the list, then the composite's result under its rebound descriptor"
            );
            assert_eq!(
                (composite.work.rebinds, composite.work.descriptors),
                (1, 3),
                "{point:?}: one pass copies the composite and its two constituents"
            );
            assert_eq!(
                shape(composite.work),
                shape(composite.synchronous_work),
                "{point:?}: nothing is walked, compared or forced again"
            );

            let apparent =
                read_across_one_retirement(point, apparent_callee_host, |_, dispatch| {
                    apparent_type_keys(dispatch).0
                });
            assert!(
                apparent.synchronous.0.contains("marker"),
                "premise: {:?}",
                apparent.synchronous
            );
            assert_eq!(
                apparent.polled, apparent.synchronous,
                "{point:?}: an apparent type"
            );
            assert_eq!(
                apparent.stages,
                (1, 12),
                "{point:?}: the apparent type's twelve reads, its anchor list read once"
            );
            assert_eq!(
                (apparent.work.rebinds, apparent.work.descriptors),
                (0, 0),
                "{point:?}"
            );
            // The strict-interface choice reads the same call list again,
            // as a later reader: the memo never serves it the retired value,
            // so that reader walks `o`'s surface, one object node, once
            // more. The anchor walk itself reads its list once.
            assert_eq!(
                shape(apparent.work),
                (
                    apparent.synchronous_work.discovery + 1,
                    apparent.synchronous_work.positions,
                    apparent.synchronous_work.forced
                ),
                "{point:?}: an apparent type"
            );
        }
    }

    /// A composite candidate's result forced before its epoch retired —
    /// the replacement landing after the result read has its result, before
    /// the overload set consumes it — is delivered to the set and consumed
    /// where it was published: the result is not forced again, nothing is
    /// copied, and the set reads what the synchronous entry reads.
    ///
    /// Dropping the delivered result for the retirement instead reads the
    /// list again and forces the result a second time.
    #[test]
    fn a_composite_result_forced_before_its_epoch_retired_is_delivered_not_forced_again() {
        use verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint;
        let run = read_across_one_retirement(
            SignaturesOfTypeBuildPoint::ResultApplied,
            union_callee_host,
            |host, dispatch| union_overload_set_keys(host, dispatch).0,
        );
        assert!(
            run.synchronous.0.contains("OverloadSet") && !run.synchronous.3,
            "premise: {:?}",
            run.synchronous
        );
        assert_eq!(run.polled, run.synchronous);
        assert_eq!(
            run.stages,
            (1, 2),
            "the list, then the composite's result, each read once"
        );
        assert_eq!(run.work.forced, 1, "the result is forced once");
        assert_eq!(
            (run.work.rebinds, run.work.descriptors),
            (0, 0),
            "the delivered result names nothing to copy"
        );
    }

    /// A callable object surface and the global `Function` interface its
    /// apparent type widens to.
    const APPARENT_CALLEE: &str =
        "declare global { interface Function { readonly marker: \"fn\"; } }\n\
                                   declare const o: { (a: string): 1 };\n";

    fn apparent_callee_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            CONTEXTUAL_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(APPARENT_CALLEE),
            crate::FileLanguage::script_ts(),
        );
        host
    }

    /// The apparent type of `o`'s object surface, and the call-signature
    /// list read its anchor walk makes.
    fn apparent_type_keys(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let callee = dispatch.execute_read(dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    CONTEXTUAL_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: "o".into(),
            },
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ));
        let QueryResult::Value(surface) = callee.value() else {
            panic!("`o` has a type: {callee:?}");
        };
        assert!(
            matches!(
                dispatch.graph_arc().node_data(*surface).as_deref(),
                Some(SemanticNodeData::Object(_))
            ),
            "`o`'s type is its object surface"
        );
        let apparent = SemanticQueryKey::ApparentType {
            base: *surface,
            context: dispatch.apparent_type_context_scoped(
                CONTEXTUAL_FILE,
                verter_type_engine::semantic_query::ApparentDemandScope::Anchored,
            ),
        };
        let list = SemanticQueryKey::SignaturesOfType {
            subject: *surface,
            kind: verter_type_engine::semantic_query::SignatureKind::Call,
            context: verter_type_engine::semantic_query::SemanticContextId::production(),
        };
        (apparent, list)
    }

    /// The type-resolution hops a cold apparent type's request records:
    /// polled on the continuation runtime, its reads the frames' needs, or
    /// built by the synchronous producer's driver, which reads each query
    /// its continuation names in place.
    #[cfg(feature = "semantic-observe")]
    fn apparent_type_hops(polled: bool) -> u64 {
        let host = apparent_callee_host();
        let (apparent, _) = apparent_type_keys(&ProjectSemanticDispatch::new(&*host));
        let ctx = verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            8,
            Arc::from(CONTEXTUAL_FILE),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            0,
        );
        let _guard =
            verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&ctx));
        let view = host.resolver_store_view_read().into_owned_view();
        let captured = crate::resolver_core::HostResolverContext::<
            verter_audit::observe::Captured,
        >::captured_for_tests(&host, &view);
        let mut dispatch = ProjectSemanticDispatch::new(&captured);
        let before = ctx
            .type_resolution_hops
            .load(std::sync::atomic::Ordering::Relaxed);
        if polled {
            let mut entry = dispatch.poll_query(apparent);
            let _ = poll_until_ready(|waker| entry.poll(waker));
        } else {
            assert!(
                dispatch.build_family_in_place_for_tests(&apparent),
                "an apparent type builds as a continuation"
            );
        }
        ctx.type_resolution_hops
            .load(std::sync::atomic::Ordering::Relaxed)
            - before
    }

    /// Each read an apparent type's continuation names counts as one
    /// dispatch intent whichever driver reads it: polled, every read is a
    /// frame's need, counted at its entry; built in place, the driver counts
    /// it where it reads it. The surface read is never counted twice.
    #[cfg(feature = "semantic-observe")]
    #[test]
    fn a_polled_apparent_type_counts_the_dispatch_intents_the_in_place_driver_counts() {
        let synchronous = apparent_type_hops(false);
        assert!(
            synchronous >= 1,
            "the apparent type's surface read is a hop"
        );
        assert_eq!(apparent_type_hops(true), synchronous);
    }

    /// The apparent type of a callable object surface whose call-signature
    /// list another task is producing returns `Pending` at the polled entry
    /// instead of waiting that producer out; its `Send` task stays pending
    /// on another thread while the producer holds. Once released it resumes
    /// at its anchor walk — the list read once, then the strict-interface
    /// lists, the global lookups and the interface's instantiation, each
    /// once — and reads exactly what the
    /// synchronous entry, which drives the same continuation in place,
    /// reads: answer, fact status, diagnostics and receipt, logical work
    /// included. Released while pending instead, it leaves nothing open and
    /// the dispatcher answers it afterwards as the synchronous entry does;
    /// warm, it runs no stage.
    #[test]
    fn an_apparent_type_whose_anchor_list_is_held_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = apparent_callee_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (apparent, _) = apparent_type_keys(&dispatch);
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(apparent);
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (1, 12),
                "the apparent type begins once, then reads its anchor list, its call and construct lists for the strict interface, looks up `CallableFunction` (its library and contributor indexes) and then `Function` (both indexes and its contributing module), and reads the interface, each once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("marker") && !synchronous.3,
            "the object surface widens to the global `Function`: {synchronous:?}"
        );

        let host = apparent_callee_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (apparent, list) = apparent_type_keys(&dispatch);
        let (tasks, leases): (Vec<_>, Vec<_>) = [
            hold_producer(&host, graph, list.clone()),
            hold_producer_with(
                &host,
                graph,
                list.clone(),
                ReadCapture::expression_for_tests(),
            ),
        ]
        .into_iter()
        .unzip();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(apparent.clone());
        let (first, frames, elsewhere) = while_held(leases, || {
            let first = entry.poll(&waker).is_pending();
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, elsewhere)
        });
        assert!(
            first,
            "the apparent type yields while its anchor list is held"
        );
        assert_eq!(frames, Some(1), "the apparent type's own frame waits");
        assert_eq!(
            elsewhere,
            Some(true),
            "its task stays pending on another thread while the producer holds"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken apparent type reads its list itself"),
        };
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (1, 12),
            "no stage runs again: every list, lookup read and the interface are each read once"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);

        let mut warm = dispatch.poll_query(apparent.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm apparent type answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(apparent);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            after,
            "a warm apparent type runs no stage"
        );
        drop(tasks);

        let host = apparent_callee_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (apparent, list) = apparent_type_keys(&dispatch);
        let (other, lease) = hold_producer(&host, graph, list);
        let mut entry = dispatch.poll_query(apparent.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(&waker).is_pending();
            drop(entry);
            pending
        });
        assert!(
            pending,
            "the apparent type yields while its anchor list is held"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(apparent);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// A class whose instance surface is its type-space half.
    const INSTANCE_CLASS: &str = "export class C { marker: \"instance\" = \"instance\"; }\n";

    fn instance_class_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            CONTEXTUAL_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(INSTANCE_CLASS),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(CONTEXTUAL_FILE)
            .expect("the instance-surface fixture has indexed input");
        host
    }

    /// `C`'s instance surface, and the type-space instantiation it reads.
    fn class_instance_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let slot = dispatch.type_slot_for(
            CONTEXTUAL_FILE,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "C",
        );
        let env = host.host_view_env_hashes_for(CONTEXTUAL_FILE);
        let surface = SemanticQueryKey::ResolveClassSurface {
            decl_slot: slot.clone(),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            side: verter_type_engine::semantic_query::ClassSurfaceSide::Instance,
            context: verter_type_engine::semantic_query::ClassSurfaceContext {
                parse_env_hash: env.parse_env_hash,
                resolve_env_hash: env.resolve_env_hash,
                mode: ProjectionMode::Shallow,
            },
        };
        let instantiate =
            SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
                slot,
                Arc::from(Vec::new().into_boxed_slice()),
                dispatch.instantiate_context_for(
                    CONTEXTUAL_FILE,
                    ProjectionReductionContext::published(ProjectionMode::Shallow),
                ),
            ));
        (surface, instantiate)
    }

    /// A class instance surface whose instantiation another task is
    /// producing returns `Pending` at the polled entry instead of waiting
    /// that producer out; its `Send` task stays pending on another thread
    /// while the producer holds. Once released it resumes at the
    /// instantiation — the declaring file read once, the instantiation once
    /// — and reads exactly what the synchronous entry, which drives the same
    /// continuation in place, reads: answer, fact status, diagnostics and
    /// receipt, logical work included. Released while pending instead, it
    /// leaves nothing open and the dispatcher answers it afterwards as the
    /// synchronous entry does; warm, it runs no stage.
    #[test]
    fn a_class_instance_surface_whose_instantiation_is_held_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = instance_class_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (surface, _) = class_instance_keys(&host, &dispatch);
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(surface);
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (1, 2),
                "the instance surface begins once, then reads its declaring file and its instantiation, each once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("marker") && !synchronous.2 && !synchronous.3,
            "the instance surface is the class's admitted complete instance type: {synchronous:?}"
        );

        let host = instance_class_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (surface, instantiate) = class_instance_keys(&host, &dispatch);
        let (tasks, leases): (Vec<_>, Vec<_>) = [
            hold_producer(&host, graph, instantiate.clone()),
            hold_producer_with(
                &host,
                graph,
                instantiate.clone(),
                ReadCapture::expression_for_tests(),
            ),
        ]
        .into_iter()
        .unzip();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(surface.clone());
        let (first, frames, elsewhere) = while_held(leases, || {
            let first = entry.poll(&waker).is_pending();
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, elsewhere)
        });
        assert!(
            first,
            "the instance surface yields while its instantiation is held"
        );
        assert_eq!(frames, Some(1), "the instance surface's own frame waits");
        assert_eq!(
            elsewhere,
            Some(true),
            "its task stays pending on another thread while the producer holds"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken instance surface reads its instantiation itself"),
        };
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (1, 2),
            "no stage runs again: the declaring file and the instantiation are each read once"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);

        let mut warm = dispatch.poll_query(surface.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm instance surface answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(surface);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            after,
            "a warm instance surface runs no stage"
        );
        drop(tasks);

        let host = instance_class_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (surface, instantiate) = class_instance_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, instantiate);
        let mut entry = dispatch.poll_query(surface.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(&waker).is_pending();
            drop(entry);
            pending
        });
        assert!(
            pending,
            "the instance surface yields while its instantiation is held"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(surface);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// A derived class whose static side inherits a base's static member
    /// and constructor.
    const STATIC_CLASSES: &str = "export class Base { static shared: \"base\" = \"base\"; constructor(seed: string) {} }\n\
                                  export class Derived extends Base { static own: \"derived\" = \"derived\"; }\n";

    fn static_class_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            CONTEXTUAL_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(STATIC_CLASSES),
            crate::FileLanguage::script_ts(),
        );
        // These fixtures hold the base-surface read, after indexed input.
        host.ensure_indexed_ready(CONTEXTUAL_FILE)
            .expect("the static-surface fixture has indexed input");
        host
    }

    /// The static surface of the class `name` declared in the contextual
    /// file.
    fn class_static_key(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        name: &str,
    ) -> SemanticQueryKey {
        let env = host.host_view_env_hashes_for(CONTEXTUAL_FILE);
        SemanticQueryKey::ResolveClassSurface {
            decl_slot: dispatch.type_slot_for(
                CONTEXTUAL_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name,
            ),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            side: verter_type_engine::semantic_query::ClassSurfaceSide::Static,
            context: verter_type_engine::semantic_query::ClassSurfaceContext {
                parse_env_hash: env.parse_env_hash,
                resolve_env_hash: env.resolve_env_hash,
                mode: ProjectionMode::Shallow,
            },
        }
    }

    /// A derived class's static surface whose base's static surface another
    /// task is producing returns `Pending` at the polled entry instead of
    /// waiting that producer out; its `Send` task stays pending on another
    /// thread while the producer holds. Once released it resumes at the
    /// base's read — its declaring file, its effective declaration and its
    /// constructor shape each read once — and reads exactly what the
    /// synchronous entry, which drives the same continuations in place,
    /// reads: answer, fact status, diagnostics and receipt, logical work
    /// included. Released while pending instead, it leaves nothing open and
    /// the dispatcher answers it afterwards as the synchronous entry does;
    /// warm, it runs no stage.
    #[test]
    fn a_class_static_surface_whose_base_surface_is_held_yields_and_reads_what_the_synchronous_entry_reads(
    ) {
        let synchronous = {
            let host = static_class_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let derived = class_static_key(&host, &dispatch, "Derived");
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(derived);
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (2, 10),
                "the derived and base static surfaces each begin once; the derived one reads its file, declaration, constructor, base, base surface and composition once, the base its file, declaration, constructor and composition once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            synchronous.0.contains("own: \"derived\"")
                && synchronous.0.contains("shared: \"base\"")
                && synchronous.0.contains("new (string) => DeclRef(Derived)")
                && !synchronous.2
                && !synchronous.3,
            "the derived static surface composes its own and its base's statics and inherits the base constructor, admitted complete: {synchronous:?}"
        );
        {
            // `typeof Derived` reads the class's static surface inside its
            // own build, where the synchronous producer's driver composes it
            // in place.
            let host = static_class_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let before = continuation_stages(&host);
            let read = dispatch.execute_via_cold_build_helper(dispatch.typeof_key_for(
                ValueRootKey {
                    scope: ScopeId::file(
                        CONTEXTUAL_FILE,
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    ),
                    name: "Derived".into(),
                },
                ProjectionReductionContext::published(ProjectionMode::Shallow),
            ));
            let after = continuation_stages(&host);
            assert_eq!(
                (after.0 - before.0, after.1 - before.1),
                (2, 10),
                "the value's static surface runs the same stages in place"
            );
            assert_eq!(
                display(&dispatch, read.value()),
                synchronous.0,
                "the in-place driver composes the same surface"
            );
        }

        let host = static_class_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let derived = class_static_key(&host, &dispatch, "Derived");
        let base = class_static_key(&host, &dispatch, "Base");
        let (tasks, leases): (Vec<_>, Vec<_>) = [
            hold_producer(&host, graph, base.clone()),
            hold_producer_with(
                &host,
                graph,
                base.clone(),
                ReadCapture::expression_for_tests(),
            ),
        ]
        .into_iter()
        .unzip();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(derived.clone());
        let (first, frames, elsewhere, held_stages) = while_held(leases, || {
            let first = entry.poll(&waker).is_pending();
            let held_stages = continuation_stages(&host);
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, elsewhere, held_stages)
        });
        assert!(
            first,
            "the derived static surface yields while its base's surface is held"
        );
        assert_eq!(
            frames,
            Some(1),
            "the derived static surface's own frame waits"
        );
        assert_eq!(
            elsewhere,
            Some(true),
            "its task stays pending on another thread while the producer holds"
        );
        assert_eq!(
            (held_stages.0 - before.0, held_stages.1 - before.1),
            (1, 4),
            "held, the derived surface stands at its base's read: its file, declaration, constructor and base classified, each once"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken static surface reads its base's surface itself"),
        };
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            (2, 10),
            "no stage runs again: the derived surface resumes at its base's read"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);

        let mut warm = dispatch.poll_query(derived.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm static surface answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(derived);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            after,
            "a warm static surface runs no stage"
        );
        drop(tasks);

        let host = static_class_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let derived = class_static_key(&host, &dispatch, "Derived");
        let base = class_static_key(&host, &dispatch, "Base");
        let (other, lease) = hold_producer(&host, graph, base);
        let mut entry = dispatch.poll_query(derived.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(&waker).is_pending();
            drop(entry);
            pending
        });
        assert!(
            pending,
            "the derived static surface yields while its base's surface is held"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(derived);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// `defineProps` over two type arguments, owned by the chain's file, and
    /// the payload operation its value is read from: the intersection of its
    /// arguments.
    fn props_payload_keys(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        owned_props_payload_keys(dispatch, TASK_FILE)
    }

    /// [`props_payload_keys`] owned by `owner`.
    fn owned_props_payload_keys(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        owner: &str,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let graph = dispatch.graph_arc();
        let args: Arc<[verter_type_engine::semantic_query::SemanticNodeId]> = Arc::from([
            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
        ]);
        let payload = SemanticQueryKey::ResolveMacroPayload {
            owner: verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                Arc::<str>::from(owner),
                verter_type_expr::TopLevelOwnerId::instance(0),
                Arc::<str>::from("<sfc-script-setup>"),
            ),
            macro_index: 0,
            macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps,
            type_args: Arc::clone(&args),
            context: verter_type_engine::semantic_query::MacroPayloadContext::new(
                Default::default(),
                ProjectionMode::Expanded,
            ),
        };
        let intersection = SemanticQueryKey::reduce_intersection_operands(graph, args);
        (payload, intersection)
    }

    /// The family continuation stages `host` ran: builds begun, and later
    /// stages resumed with a read.
    fn continuation_stages(host: &crate::VerterHost) -> (u64, u64) {
        let stats = host.project_type_store().semantic_graph().stats_snapshot();
        (
            stats.continuation_builds_begun,
            stats.continuation_stages_resumed,
        )
    }

    /// A macro payload — the metadata root's own query — whose payload
    /// operation another task is producing returns `Pending` with its first
    /// stage retained instead of waiting that producer out, and its `Send`
    /// task stays pending on another thread while the producer holds. Once
    /// the producer goes away the payload finishes from what its first stage
    /// kept, reading its owner once, and delivers exactly what the
    /// synchronous entry reads on a fresh host: answer, fact status,
    /// diagnostics and receipt, logical work included. Warm, the polled and
    /// synchronous entries answer alike without running a stage.
    #[test]
    fn a_macro_payload_waiting_on_its_payload_operation_yields_and_finishes_from_its_retained_stage(
    ) {
        let synchronous = {
            let host = chain_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (payload, _) = props_payload_keys(&dispatch);
            let read = dispatch.execute_via_cold_build_helper(payload);
            assert_eq!(
                continuation_stages(&host),
                (2, 2),
                "the payload and its intersection each begin once; the owner and the payload are read once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "the payload is complete and priced: {synchronous:?}"
        );

        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (payload, intersection) = props_payload_keys(&dispatch);
        let (other, lease) = hold_producer(&host, graph, intersection);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut entry = dispatch.poll_query(payload.clone());
        let (first, frames, elsewhere) = while_held(lease, || {
            let first = entry.poll(&waker).is_pending();
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, elsewhere)
        });
        assert!(
            first,
            "the payload yields while another task produces its intersection"
        );
        assert_eq!(frames, Some(1), "the payload's own frame waits");
        assert_eq!(
            elsewhere,
            Some(true),
            "its task stays pending on another thread while the producer holds"
        );
        assert!(
            counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "the producer going away wakes the root"
        );
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken payload produces its intersection itself"),
        };
        drop(entry);
        assert_eq!(
            continuation_stages(&host),
            (2, 2),
            "no stage runs again: the owner and the payload are read once"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());

        let mut warm = dispatch.poll_query(payload.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm payload answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(payload);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            (2, 2),
            "a warm payload runs no stage"
        );
        drop(other);
    }

    /// `string & ("a" & string) & string`, its middle operand a nested
    /// subgroup the reduction reads, and that subgroup's own reduction.
    fn stepped_intersection_keys(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        use verter_type_engine::semantic_query::{
            IntersectionInputRef, IntersectionPurpose, IntersectionTerm, LiteralValue,
            SemanticContextId,
        };
        let graph = dispatch.graph_arc();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let literal =
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("a".into())));
        let subgroup = IntersectionInputRef::from_operands(graph, &[literal, string]);
        let purpose = IntersectionPurpose::CheckerReduction;
        let reduction = SemanticQueryKey::ReduceIntersection {
            input: IntersectionInputRef::from_steps(
                graph,
                &[
                    IntersectionTerm::Value(string),
                    IntersectionTerm::EvaluateSubgroup {
                        input: subgroup.clone(),
                        purpose,
                    },
                    IntersectionTerm::Value(string),
                ],
            ),
            purpose,
            context: SemanticContextId::production(),
        };
        let nested = SemanticQueryKey::ReduceIntersection {
            input: subgroup,
            purpose,
            context: SemanticContextId::production(),
        };
        (reduction, nested)
    }

    /// An intersection reduction whose nested subgroup another task is
    /// reducing returns `Pending` at the polled entry with the operands it
    /// took before the subgroup retained, instead of waiting that producer
    /// out; its `Send` task stays pending on another thread while the
    /// producer holds. Once the producer goes away the reduction takes the
    /// subgroup's answer and its remaining operands, running no stage again,
    /// and delivers exactly what the synchronous entry — which drives the
    /// same continuation in place — reads on a fresh host: answer, fact
    /// status, diagnostics and receipt, logical work included.
    #[test]
    fn an_intersection_reduction_waiting_on_a_nested_subgroup_yields_and_resumes_after_it() {
        let synchronous = {
            let host = chain_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let _builder = dispatch.enter_construction_scope();
            let (reduction, _) = stepped_intersection_keys(&dispatch);
            let read = dispatch.execute_via_cold_build_helper(reduction);
            assert_eq!(
                continuation_stages(&host),
                (2, 1),
                "the reduction and its subgroup each begin once; the reduction resumes once: {:?}",
                read_summary(&dispatch, &read)
            );
            read_summary(&dispatch, &read)
        };
        assert_eq!(synchronous.0, "\"a\"", "{synchronous:?}");
        assert!(synchronous.5.is_some(), "the reduction is priced");

        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _builder = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (reduction, nested) = stepped_intersection_keys(&dispatch);
        let (other, lease) = hold_producer(&host, graph, nested);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let mut entry = dispatch.poll_query(reduction.clone());
        let (first, held_stages, elsewhere) = while_held(lease, || {
            let first = entry.poll(&waker).is_pending();
            let held_stages = continuation_stages(&host);
            let elsewhere = entry.task().map(|task| {
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, held_stages, elsewhere)
        });
        assert!(
            first,
            "the reduction yields while another task reduces its subgroup"
        );
        assert_eq!(
            held_stages,
            (1, 0),
            "the reduction's first stage ran once and holds its operands"
        );
        assert_eq!(elsewhere, Some(true));
        assert!(counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        let read = match entry.poll(&waker) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("the woken reduction reduces its subgroup itself"),
        };
        drop(entry);
        assert_eq!(continuation_stages(&host), (2, 1), "no stage runs again");
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());

        let mut warm = dispatch.poll_query(reduction.clone());
        let Poll::Ready(warm_read) = warm.poll(&waker) else {
            panic!("a warm reduction answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(reduction);
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
        assert_eq!(
            continuation_stages(&host),
            (2, 1),
            "a warm read runs no stage"
        );
        drop(other);
    }

    /// A file the workspace holds and the host has not loaded.
    const UNLOADED_OWNER: &str = "/semantic-task/owner.ts";

    /// A host whose workspace holds [`UNLOADED_OWNER`], unloaded.
    fn unloaded_owner_host() -> Arc<crate::VerterHost> {
        unloaded_files_host(&[(UNLOADED_OWNER, "export interface Props { a: string }\n")])
    }

    /// A host whose workspace holds `files`, none of them loaded.
    fn unloaded_files_host(files: &[(&str, &str)]) -> Arc<crate::VerterHost> {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        for (path, source) in files {
            workspace.inject_file((*path).to_owned(), Arc::from(*source));
        }
        Arc::new(crate::VerterHost::new(
            crate::types::HostConfig {
                analysis_level: crate::types::AnalysisLevel::Full,
                audit_enabled: true,
                footprint_capture: false,
                ..crate::types::HostConfig::default()
            },
            workspace,
        ))
    }

    /// Wakes the thread that polls.
    struct ThreadWaker(std::thread::Thread);

    impl std::task::Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Counts its wakes and unparks the thread that waits on them, so a
    /// test can wait for the wake a released load delivers before it polls
    /// again: a poll re-registers its own waker, and a poll made before the
    /// load completed would take that wake instead.
    struct CountingThreadWaker {
        wakes: std::sync::atomic::AtomicUsize,
        thread: std::thread::Thread,
    }

    impl std::task::Wake for CountingThreadWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wakes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.thread.unpark();
        }
    }

    impl CountingThreadWaker {
        fn for_current_thread() -> Arc<Self> {
            Arc::new(Self {
                wakes: std::sync::atomic::AtomicUsize::new(0),
                thread: std::thread::current(),
            })
        }

        /// Park until woken at least once, failing past [`HELD_DEADLINE`]
        /// with `expectation` instead of hanging.
        fn wait_for_wake(&self, expectation: &str) {
            let deadline = std::time::Instant::now() + HELD_DEADLINE;
            while self.wakes.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                let now = std::time::Instant::now();
                assert!(now < deadline, "{expectation}");
                std::thread::park_timeout(deadline - now);
            }
        }
    }

    /// Poll until ready, parking between polls until woken, failing past
    /// [`HELD_DEADLINE`] instead of hanging.
    pub(super) fn poll_until_ready<T>(mut poll: impl FnMut(&Waker) -> Poll<T>) -> T {
        let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
        let deadline = std::time::Instant::now() + HELD_DEADLINE;
        loop {
            if let Poll::Ready(value) = poll(&waker) {
                return value;
            }
            let now = std::time::Instant::now();
            assert!(now < deadline, "the released load wakes its waiter");
            std::thread::park_timeout(deadline - now);
        }
    }

    /// Releases a held source read when dropped, so a failed assertion
    /// cannot leave a scheduler worker held.
    struct HeldSource<'h>(&'h crate::host_test_force::SourceLoadGate);

    impl Drop for HeldSource<'_> {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    /// A macro payload whose owning file is still loading — its scheduler
    /// source read held — returns `Pending` at the polled entry instead of
    /// driving or waiting out the load, with its first stage retained; its
    /// `Send` task stays pending on another thread. Once the read goes on,
    /// the load completes and wakes the root, which reads its owner once and
    /// delivers exactly what the synchronous entry, which loads the owner in
    /// place, reads on a fresh host: answer, fact status, diagnostics and
    /// receipt, logical work included.
    #[test]
    fn a_macro_payload_whose_owner_is_loading_yields_and_reads_it_once_loaded() {
        let synchronous = {
            let host = unloaded_owner_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (payload, _) = owned_props_payload_keys(&dispatch, UNLOADED_OWNER);
            let read = dispatch.execute_via_cold_build_helper(payload);
            assert!(
                host.scheduler.try_get_source(UNLOADED_OWNER).is_some(),
                "the synchronous entry loads the owner"
            );
            assert_eq!(
                continuation_stages(&host),
                (2, 2),
                "the payload and its intersection begin once; the owner and the payload are read once"
            );
            read_summary(&dispatch, &read)
        };
        assert!(
            !synchronous.2 && !synchronous.3 && synchronous.5.is_some(),
            "a loaded owner roots a complete, cacheable, priced payload: {synchronous:?}"
        );

        let host = unloaded_owner_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (payload, _) = owned_props_payload_keys(&dispatch, UNLOADED_OWNER);
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(UNLOADED_OWNER);
        let held = HeldSource(&gate);
        let mut entry = dispatch.poll_query(payload.clone());
        let counter = CountingThreadWaker::for_current_thread();
        let counting = Waker::from(Arc::clone(&counter));
        let first = entry.poll(&counting).is_pending();
        let held_stages = continuation_stages(&host);
        let loaded_while_held = host.scheduler.try_get_source(UNLOADED_OWNER).is_some();
        let elsewhere = entry.task().map(|task| {
            assert_send(task);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| task.poll(&counting).is_pending())
                    .join()
                    .expect("the pending poll returns")
            })
        });
        drop(held);
        assert!(first, "the payload yields while its owner loads");
        assert_eq!(held_stages, (1, 0), "the first stage ran once and waits");
        assert!(!loaded_while_held, "the owner's source read is still held");
        assert_eq!(elsewhere, Some(true));
        counter.wait_for_wake("the completed load wakes the waiting poll");
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        assert_eq!(
            continuation_stages(&host),
            (2, 2),
            "no stage runs again: the owner is read once after its load"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    fn cycle_gate_host() -> Arc<crate::VerterHost> {
        unloaded_files_host(&[(
            UNLOADED_OWNER,
            "export interface Props { child: Child } export interface Child { value: string }",
        )])
    }

    fn cycle_gate_keys(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let slot = |name: &str| {
            dispatch.type_slot_for(
                UNLOADED_OWNER,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name,
            )
        };
        let root = dispatch.materialization_cycle_gate_key_for(
            &verter_type_engine::semantic_query::DeclIdentity {
                canonical_id: UNLOADED_OWNER.into(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [0; 16],
                decl_name: "Props".into(),
            },
        );
        let child =
            SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
                slot("Child"),
                Arc::from([]),
                dispatch.instantiate_context_for(
                    UNLOADED_OWNER,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Skeleton,
                    ),
                ),
            ));
        (root, child)
    }

    #[test]
    fn a_cycle_gate_retains_completed_hops_while_a_nested_declaration_is_held() {
        let synchronous = {
            let host = cycle_gate_host();
            host.ensure_indexed_ready(UNLOADED_OWNER).unwrap();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (root, _) = cycle_gate_keys(&dispatch);
            let read = dispatch.execute_via_cold_build_helper(root);
            read_summary(&dispatch, &read)
        };
        for abandon in [false, true] {
            let host = cycle_gate_host();
            host.ensure_indexed_ready(UNLOADED_OWNER).unwrap();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let graph = host.project_type_store().semantic_graph();
            let (root, child) = cycle_gate_keys(&dispatch);
            let (other, lease) = hold_producer(&host, graph, child);
            let counter = Arc::new(CountingWaker::default());
            let waker = Waker::from(Arc::clone(&counter));
            let mut entry = Some(dispatch.poll_query(root));
            let (pending, progress, repeated) = while_held(lease, || {
                let pending = entry.as_mut().unwrap().poll(&waker).is_pending();
                let progress = continuation_stages(&host);
                let repeated = entry.as_mut().unwrap().task().map(|task| {
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .unwrap()
                    })
                });
                assert_eq!(
                    continuation_stages(&host),
                    progress,
                    "completed hops stay retained"
                );
                if abandon {
                    drop(entry.take());
                }
                (pending, progress, repeated)
            });
            assert!(pending, "a cold cycle gate yields at its nested Child hop");
            assert_eq!(repeated, Some(true));
            assert_eq!(
                progress,
                (1, 2),
                "owner and Props completed before Child waits"
            );
            let read = entry.take().map(|mut entry| {
                assert!(counter.0.load(std::sync::atomic::Ordering::SeqCst) > 0);
                poll_until_ready(|waker| entry.poll(waker))
            });
            drop(entry);
            if let Some(read) = read {
                assert_eq!(
                    continuation_stages(&host),
                    (1, 3),
                    "no completed hop restarts"
                );
                assert_eq!(read_summary(&dispatch, &read), synchronous);
            }
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            assert!(graph.resident_flight_keys_for_tests().is_empty());
            drop(other);
        }
    }

    #[test]
    fn a_cycle_gate_with_present_source_yields_for_indexing() {
        let synchronous = {
            let host = cycle_gate_host();
            assert!(host.ensure_loaded(UNLOADED_OWNER));
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (root, _) = cycle_gate_keys(&dispatch);
            let read = dispatch.execute_via_cold_build_helper(root);
            read_summary(&dispatch, &read)
        };
        let host = cycle_gate_host();
        assert!(host.ensure_loaded(UNLOADED_OWNER));
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let (root, _) = cycle_gate_keys(&dispatch);
        let mut entry = dispatch.poll_query(root);
        {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            assert!(entry.poll(Waker::noop()).is_pending());
            assert!(entry.poll(Waker::noop()).is_pending());
        }
        held.release();
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
    }

    fn declaration_key(file: &str) -> SemanticQueryKey {
        use verter_type_engine::semantic_query::{BinderScopeId, ResolveDeclKey, ScopeId};
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: file.into(),
                owner,
                local_scope: None,
                binder_scope_id: BinderScopeId::file_scope(owner),
            },
            name: "Props".into(),
        })
    }

    #[test]
    fn a_declaration_with_present_source_yields_until_its_index_is_ready() {
        let host = unloaded_owner_host();
        assert!(host.ensure_loaded(UNLOADED_OWNER));
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let mut entry = dispatch.poll_query(declaration_key(UNLOADED_OWNER));
        let counter = CountingThreadWaker::for_current_thread();
        let waker = Waker::from(Arc::clone(&counter));
        {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            assert!(entry.poll(&waker).is_pending());
            assert!(entry.poll(&waker).is_pending());
        }
        held.release();
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        let synchronous = dispatch.execute_via_cold_build_helper(declaration_key(UNLOADED_OWNER));
        assert_eq!(
            read_summary(&dispatch, &read),
            read_summary(&dispatch, &synchronous)
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
    }

    #[test]
    fn a_cancelled_index_load_is_not_retried_on_the_polling_thread() {
        let host = unloaded_owner_host();
        assert!(host.ensure_loaded(UNLOADED_OWNER));
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let mut entry = dispatch.poll_query(declaration_key(UNLOADED_OWNER));
        {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            assert!(entry.poll(Waker::noop()).is_pending());
            host.cooperative_drive.cancellation().cancel();
        }
        // The owner shards stay held: answering from anywhere but the
        // retained outcome would park this thread.
        let read = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            poll_until_ready(|waker| entry.poll(waker))
        };
        drop(entry);
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Miss)
            ),
            "a failed load answers as a failed synchronous read"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
        held.release();
    }

    /// A source the scheduler drops after an indexed load saw it is one more
    /// owned load of that computation: polls stay `Pending` on the reload and
    /// no reader drives it. Dropping or cancelling the load while it waits
    /// leaves the file's flight claimable.
    #[test]
    fn an_indexed_load_awaits_a_source_lost_before_its_first_poll() {
        use verter_type_engine::resolver_core::request_ports::{
            IndexedLoadEnd, IndexedLoadOutcome, IndexedReadiness,
        };

        enum End {
            Reload,
            Drop,
            Cancel,
        }
        for end in [End::Reload, End::Drop, End::Cancel] {
            let host = unloaded_owner_host();
            assert!(host.ensure_loaded(UNLOADED_OWNER));
            let blocking_before = host
                .test_force
                .blocking_loads_begun
                .load(std::sync::atomic::Ordering::SeqCst);
            let IndexedReadiness::Loading(mut load) = host.begin_indexed_ready_load(UNLOADED_OWNER)
            else {
                panic!("the index is cold");
            };
            host.scheduler.close_file(UNLOADED_OWNER);
            let gate = Arc::clone(&host.test_force.source_load_gate);
            gate.hold(UNLOADED_OWNER);
            let held = HeldSource(&gate);
            {
                let _forbid = verter_execution::blocking::WaitProbe::forbid();
                assert!(load.poll(Waker::noop()).is_pending());
                assert!(load.poll(Waker::noop()).is_pending());
            }
            assert_eq!(
                host.test_force
                    .blocking_loads_begun
                    .load(std::sync::atomic::Ordering::SeqCst),
                blocking_before,
                "the reload is never driven inside the poll"
            );
            match end {
                End::Reload => {
                    drop(held);
                    let outcome = poll_until_ready(|waker| {
                        let _forbid = verter_execution::blocking::WaitProbe::forbid();
                        load.poll(waker)
                    });
                    assert_eq!(outcome, IndexedLoadOutcome::Integrated);
                    let delivered = load.take_indexed_input().expect("the reload indexes it");
                    assert!(delivered.store_published);
                }
                End::Drop => {
                    drop(load);
                    assert_eq!(host.task_registry.wait_count_for_tests(), 0);
                    drop(held);
                }
                End::Cancel => {
                    host.cooperative_drive.cancellation().cancel();
                    let outcome = poll_until_ready(|waker| {
                        let _forbid = verter_execution::blocking::WaitProbe::forbid();
                        load.poll(waker)
                    });
                    assert_eq!(
                        outcome,
                        IndexedLoadOutcome::NotIntegrated(IndexedLoadEnd::Cancelled)
                    );
                    drop(load);
                    assert_eq!(host.task_registry.wait_count_for_tests(), 0);
                    drop(held);
                    continue;
                }
            }
            assert!(
                host.ensure_indexed_ready_serve(UNLOADED_OWNER).is_some(),
                "the file stays claimable"
            );
            assert_eq!(host.task_registry.wait_count_for_tests(), 0);
        }
    }

    /// A reader's consumed load answers availability on both request adapters
    /// exactly as it answers a read: an end is no input and a delivery is the
    /// input, neither materialized again while the owner shards are held.
    #[test]
    fn availability_consumes_a_reader_s_load_without_materializing() {
        use verter_type_engine::resolver_core::request_ports::{
            with_consumed_loads_for_tests, ConsumedLoad, IndexedInputs, IndexedLoadEnd,
            IndexedLoadOutcome, IndexedReadiness,
        };

        // The delivery comes from another host, so this one's index stays
        // cold: an answer from anywhere but the delivery parks on the shards.
        let delivered = {
            let host = unloaded_owner_host();
            assert!(host.ensure_loaded(UNLOADED_OWNER));
            let IndexedReadiness::Loading(mut load) = host.begin_indexed_ready_load(UNLOADED_OWNER)
            else {
                panic!("the index is cold");
            };
            assert_eq!(
                poll_until_ready(|waker| load.poll(waker)),
                IndexedLoadOutcome::Integrated
            );
            load.take_indexed_input().expect("the load delivers")
        };
        let host = unloaded_owner_host();
        assert!(host.ensure_loaded(UNLOADED_OWNER));
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let view = host.resolver_store_view_read().into_owned_view();
        let ctx =
            crate::resolver_core::HostResolverContext::<verter_audit::observe::Uncaptured>::new(
                &host,
                &view,
                Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
            );
        let file: Arc<str> = Arc::from(UNLOADED_OWNER);
        let _forbid = verter_execution::blocking::WaitProbe::forbid();
        let mut ended = vec![(
            Arc::clone(&file),
            ConsumedLoad::End(IndexedLoadEnd::Cancelled),
        )];
        with_consumed_loads_for_tests(&mut ended, || {
            assert!(!IndexedInputs::source_is_available(&*host, UNLOADED_OWNER));
            assert!(!IndexedInputs::source_is_available(&ctx, UNLOADED_OWNER));
        });
        // The frame keeps the file as its key names it, normalized or not.
        let alias = UNLOADED_OWNER.strip_suffix(".ts").expect("a script owner");
        assert_eq!(host.normalized_analysis_canonical(alias), UNLOADED_OWNER);
        for file in [file, Arc::from(alias)] {
            let mut delivery = vec![(file, ConsumedLoad::Indexed(delivered.clone()))];
            with_consumed_loads_for_tests(&mut delivery, || {
                assert!(IndexedInputs::source_is_available(&*host, UNLOADED_OWNER));
                assert!(IndexedInputs::source_is_available(&ctx, UNLOADED_OWNER));
            });
        }
        drop(_forbid);
        held.release();
    }

    #[test]
    fn a_declaration_preserves_a_fenced_index_delivery_without_rebuilding_it() {
        let host = || {
            let host = unloaded_owner_host();
            assert!(host.ensure_loaded(UNLOADED_OWNER));
            host.test_force
                .force_indexed_ready_serve_fence_for_tests
                .store(true, std::sync::atomic::Ordering::Relaxed);
            host
        };
        let synchronous = {
            let host = host();
            let view = host.resolver_store_view_read().into_owned_view();
            let ctx = crate::resolver_core::HostResolverContext::new(
                &host,
                &view,
                Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
            );
            let dispatch = ProjectSemanticDispatch::new(&ctx);
            let read = dispatch.execute_via_cold_build_helper(declaration_key(UNLOADED_OWNER));
            read_summary(&dispatch, &read)
        };
        let host = host();
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let view = host.resolver_store_view_read().into_owned_view();
        let ctx = crate::resolver_core::HostResolverContext::new(
            &host,
            &view,
            Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
        );
        let mut dispatch = ProjectSemanticDispatch::new(&ctx);
        let mut entry = dispatch.poll_query(declaration_key(UNLOADED_OWNER));
        {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            assert!(entry.poll(Waker::noop()).is_pending());
        }
        held.release();
        let read = poll_until_ready(|waker| {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(waker)
        });
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
    }

    #[test]
    fn a_declaration_whose_source_is_loading_yields_and_releases_its_retained_load() {
        let synchronous = {
            let host = unloaded_owner_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(declaration_key(UNLOADED_OWNER));
            read_summary(&dispatch, &read)
        };
        for abandon in [false, true] {
            let host = unloaded_owner_host();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let graph = host.project_type_store().semantic_graph();
            let gate = Arc::clone(&host.test_force.source_load_gate);
            gate.hold(UNLOADED_OWNER);
            let held = HeldSource(&gate);
            let mut entry = Some(dispatch.poll_query(declaration_key(UNLOADED_OWNER)));
            let counter = CountingThreadWaker::for_current_thread();
            let waker = Waker::from(Arc::clone(&counter));
            let first = while_held(held, || {
                let first = entry.as_mut().unwrap().poll(&waker).is_pending();
                if abandon {
                    drop(entry.take());
                }
                first
            });
            assert!(
                first,
                "the declaration yields before its source is released"
            );
            let read = entry.take().map(|mut entry| {
                counter.wait_for_wake("the source wakes the declaration reader");
                poll_until_ready(|waker| entry.poll(waker))
            });
            drop(entry);
            if let Some(read) = read {
                assert_eq!(read_summary(&dispatch, &read), synchronous);
            }
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            assert!(graph.resident_flight_keys_for_tests().is_empty());
        }
    }

    #[test]
    fn an_imported_declaration_yields_while_its_defining_source_loads() {
        const IMPORTER: &str = "/semantic-task/decl-importer.ts";
        let host = || {
            let host = unloaded_files_host(&[
                (UNLOADED_OWNER, "export interface Props { a: string }\n"),
                (
                    IMPORTER,
                    "import { Props } from './owner'; export { Props };",
                ),
            ]);
            assert!(host.ensure_loaded(IMPORTER));
            host
        };
        let synchronous = {
            let host = host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(declaration_key(IMPORTER));
            read_summary(&dispatch, &read)
        };
        for abandon in [false, true] {
            let host = host();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let graph = host.project_type_store().semantic_graph();
            let gate = Arc::clone(&host.test_force.source_load_gate);
            gate.hold(UNLOADED_OWNER);
            let mut entry = Some(dispatch.poll_query(declaration_key(IMPORTER)));
            let pending = while_held(HeldSource(&gate), || {
                let pending = entry.as_mut().unwrap().poll(Waker::noop()).is_pending();
                if abandon {
                    drop(entry.take());
                }
                pending
            });
            assert!(
                pending,
                "a routed declaration yields before its source is released"
            );
            let read = entry
                .take()
                .map(|mut entry| poll_until_ready(|waker| entry.poll(waker)));
            drop(entry);
            if let Some(read) = read {
                assert_eq!(read_summary(&dispatch, &read), synchronous);
            }
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            assert!(graph.resident_flight_keys_for_tests().is_empty());
        }
    }

    /// A declaration lookup of a name its scope imports and re-exports
    /// (`import { Props } from './owner'; export { Props }`) is the scope's
    /// own identity to the resolver: it answers the scope's placeholder from
    /// the scope's inventory alone. The polled entry answers it on its first
    /// poll, under a forbidding probe, with the owner's index still cold on
    /// held shards — it reads the import's target only when a consumer
    /// instantiates the placeholder, as the synchronous entry does — and
    /// leaves nothing open. (Before the route became the lookup's own
    /// transitions, the frame preloaded the scope's import targets ahead of
    /// the build and waited on the owner's index for an answer that never
    /// read it.)
    #[test]
    fn an_imported_declaration_answers_its_scope_without_indexing_its_import() {
        const IMPORTER: &str = "/semantic-task/decl-importer.ts";
        let make_host = || {
            let host = unloaded_files_host(&[
                (UNLOADED_OWNER, "export interface Props { a: string }"),
                (
                    IMPORTER,
                    "import { Props } from './owner'; export { Props };",
                ),
            ]);
            host.ensure_indexed_ready(IMPORTER).unwrap();
            assert!(host.ensure_loaded(UNLOADED_OWNER));
            host
        };
        let synchronous = {
            let host = make_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(declaration_key(IMPORTER));
            read_summary(&dispatch, &read)
        };
        let host = make_host();
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let mut entry = dispatch.poll_query(declaration_key(IMPORTER));
        let read = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            match entry.poll(Waker::noop()) {
                Poll::Ready(read) => read,
                Poll::Pending => {
                    panic!("the lookup answers from its scope without the import's index")
                }
            }
        };
        drop(entry);
        held.release();
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(host.task_registry.wait_count_for_tests(), 0);
    }

    #[test]
    fn a_local_declaration_does_not_load_its_unrelated_imports() {
        const LOCAL: &str = "/semantic-task/local-decl.ts";
        let host = unloaded_files_host(&[
            (UNLOADED_OWNER, "export interface Imported { a: string }"),
            (
                LOCAL,
                "import { Imported } from './owner'; export interface Props { b: number }",
            ),
        ]);
        assert!(host.ensure_loaded(LOCAL));
        host.ensure_indexed_ready(LOCAL)
            .expect("local headers are ready before the unrelated import is held");
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(UNLOADED_OWNER);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let mut entry = dispatch.poll_query(declaration_key(LOCAL));
        let ready = while_held(HeldSource(&gate), || entry.poll(Waker::noop()).is_ready());
        drop(entry);
        assert!(ready, "local headers answer without preparing imports");
        assert!(host.scheduler.try_get_source(UNLOADED_OWNER).is_none());
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
    }

    /// A file the workspace does not hold: its load fails.
    const MISSING_OWNER: &str = "/semantic-task/missing.ts";

    /// `C`'s instance surface in `file`, unscoped by a loaded declaration.
    fn class_instance_surface_in(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        file: &str,
    ) -> SemanticQueryKey {
        let env = host.host_view_env_hashes_for(file);
        SemanticQueryKey::ResolveClassSurface {
            decl_slot: dispatch.type_slot_for(
                file,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "C",
            ),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            side: verter_type_engine::semantic_query::ClassSurfaceSide::Instance,
            context: verter_type_engine::semantic_query::ClassSurfaceContext {
                parse_env_hash: env.parse_env_hash,
                resolve_env_hash: env.resolve_env_hash,
                mode: ProjectionMode::Shallow,
            },
        }
    }

    /// A reader whose file's load fails retains that terminal outcome:
    /// the polled entry submits the load once, waits on it, and — the load
    /// failed — resumes from its retained stage without the file, answering
    /// in bounded polls exactly what the synchronous entry, whose in-place
    /// load fails the same way, reads. Repeated reads of the missing file
    /// never fall back to the synchronous loader. That
    /// holds for a family continuation (a macro payload's owner, a class's
    /// declaring file), for an instantiation or a `typeof` frame that waited
    /// on its file before its build began, and for a class's surface whose
    /// instantiation is such a frame. Every reader leaves nothing open.
    ///
    /// Restarted from its load stage after the failure instead, a family
    /// begins the same load again at every resume and never answers. Consuming
    /// the failure only once lets a later scope read retry synchronously;
    /// total submissions can still match the synchronous entry, so count
    /// synchronous starts separately from owned loads.
    #[test]
    fn family_load_failure_is_terminal() {
        type Key = fn(
            &crate::VerterHost,
            &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        ) -> SemanticQueryKey;
        let submitted = |host: &crate::VerterHost| {
            host.test_force
                .loads_begun
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        let blocking = |host: &crate::VerterHost| {
            host.test_force
                .blocking_loads_begun
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        let cases: [(&str, usize, Key); 6] = [
            ("a declaration's source", 1, |_, _| {
                declaration_key(MISSING_OWNER)
            }),
            // The declaring file's load fails once; the declaration reads
            // the static surface then makes at its port consume that
            // failure instead of loading the file again.
            ("a class's static declaring file", 1, |host, dispatch| {
                class_static_surface_in(host, dispatch, MISSING_OWNER)
            }),
            ("an instantiation's declaring file", 1, |_, dispatch| {
                missing_props_key(dispatch)
            }),
            ("a typeof's value file", 1, |_, dispatch| {
                file_typeof_key(dispatch, MISSING_OWNER, "value")
            }),
            ("a macro payload's owner", 1, |_, dispatch| {
                owned_props_payload_keys(dispatch, MISSING_OWNER).0
            }),
            // The class and its Instantiate child each own a source read.
            ("a class's declaring file", 2, |host, dispatch| {
                class_instance_surface_in(host, dispatch, MISSING_OWNER)
            }),
        ];
        for (case, expected_loads, key) in cases {
            let (synchronous, synchronous_loads) = {
                let host = unloaded_files_host(&[]);
                let dispatch = ProjectSemanticDispatch::new(&*host);
                let key = key(&host, &dispatch);
                let before = submitted(&host);
                let read = dispatch.execute_via_cold_build_helper(key);
                assert!(
                    blocking(&host) > 0,
                    "{case}: the synchronous control loads in place"
                );
                (read_summary(&dispatch, &read), submitted(&host) - before)
            };
            let host = unloaded_files_host(&[]);
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let graph = host.project_type_store().semantic_graph();
            let key = key(&host, &dispatch);
            let before = submitted(&host);
            let gate = Arc::clone(&host.test_force.source_load_gate);
            gate.hold(MISSING_OWNER);
            let held = HeldSource(&gate);
            let mut entry = dispatch.poll_query(key);
            let counter = CountingThreadWaker::for_current_thread();
            let waker = Waker::from(Arc::clone(&counter));
            let (first, held_loads) = while_held(held, || {
                (entry.poll(&waker).is_pending(), submitted(&host) - before)
            });
            assert!(first, "{case}: a held missing source yields before release");
            assert_eq!(held_loads, 1, "{case}: the poll reached the held source");
            counter.wait_for_wake("the failed load wakes its reader");
            let read = poll_until_ready(|waker| entry.poll(waker));
            drop(entry);
            assert!(
                host.scheduler.try_get_source(MISSING_OWNER).is_none(),
                "{case}: premise: the load failed"
            );
            let polled_loads = submitted(&host) - before;
            assert_eq!(
                blocking(&host),
                0,
                "{case}: no synchronous retry after the failed preload"
            );
            assert_eq!(read_summary(&dispatch, &read), synchronous, "{case}");
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            assert!(graph.resident_flight_keys_for_tests().is_empty());
            assert!(
                synchronous_loads >= 1,
                "{case}: premise: the synchronous entry submits the failed load"
            );
            assert!(
                synchronous_loads >= polled_loads,
                "{case}: the polled entry submits no extra loads: {polled_loads} vs {synchronous_loads}"
            );
            assert_eq!(
                polled_loads, expected_loads,
                "{case}: one owned load per reader"
            );
            // The retained failure belongs to the finished frame, not the host:
            // an independent request may try loading the missing file again.
            let before = submitted(&host);
            assert!(!host.ensure_loaded(MISSING_OWNER));
            assert_eq!(
                submitted(&host) - before,
                1,
                "{case}: no failure leaks out of the frame"
            );
        }
    }

    /// The instantiation of a `Props` the missing file would declare.
    fn missing_props_key(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
            dispatch.type_slot_for(
                MISSING_OWNER,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Props",
            ),
            Arc::from([]),
            dispatch.instantiate_context_for(
                MISSING_OWNER,
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            ),
        ))
    }

    /// A macro payload released while its owner loads withdraws only its own
    /// wait: nothing stays open, the load it began goes on and integrates the
    /// owner for the next demand, and the request then answers the payload
    /// exactly as the synchronous entry does on a fresh host.
    #[test]
    fn a_macro_payload_released_while_its_owner_loads_leaves_nothing_open() {
        let synchronous = {
            let host = unloaded_owner_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (payload, _) = owned_props_payload_keys(&dispatch, UNLOADED_OWNER);
            read_summary(&dispatch, &dispatch.execute_via_cold_build_helper(payload))
        };
        let host = unloaded_owner_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (payload, _) = owned_props_payload_keys(&dispatch, UNLOADED_OWNER);
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(UNLOADED_OWNER);
        let held = HeldSource(&gate);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let mut entry = dispatch.poll_query(payload.clone());
        let first = entry.poll(&waker).is_pending();
        drop(entry);
        let open = dispatch.open_request_state_for_tests();
        let waits = graph.wait_graph_counts_for_tests().1;
        drop(held);
        assert!(first, "the payload yields while its owner loads");
        assert_eq!(open, NOTHING_OPEN);
        assert_eq!(waits, 0, "the released payload holds no wait");
        let read = dispatch.execute_via_cold_build_helper(payload);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    type ValueRead = verter_type_engine::semantic_query::CacheRead<QueryResult<SemanticQueryValue>>;

    /// What [`read_summary`] reports of a read.
    type ReadSummary = (String, String, bool, bool, Vec<String>, Option<String>);

    type HostDispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;

    /// Resume `entry`'s pending task on another thread until it completes —
    /// parking there between polls until woken — and deliver its answer on
    /// this thread.
    fn resume_elsewhere(
        entry: &mut verter_type_engine::project_semantic_dispatch::SemanticQueryPoll<
            '_,
            '_,
            crate::resolver_core::HostCapabilities,
        >,
    ) -> ValueRead {
        let task = entry.task().expect("a pending root has a task");
        std::thread::scope(|scope| {
            scope
                .spawn(|| poll_until_ready(|waker| task.poll(waker)))
                .join()
                .expect("the released task completes on another thread")
        });
        match entry.poll(Waker::noop()) {
            Poll::Ready(read) => read,
            Poll::Pending => panic!("a task completed elsewhere delivers on its entry's thread"),
        }
    }

    /// A component whose first macro declares its emits.
    const EMITS_OWNER: &str = "/semantic-task/emits.vue";

    fn emits_owner_host() -> Arc<crate::VerterHost> {
        let host = chain_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            EMITS_OWNER,
            "<script setup lang=\"ts\">defineEmits<{ change: [id: number] }>()</script>\n",
            crate::LanguageRegistry::global()
                .classify_static(EMITS_OWNER)
                .static_resolution(),
        );
        let _ = host
            .ensure_indexed_ready(EMITS_OWNER)
            .expect("the component's indexed input materialises");
        host
    }

    /// A `defineEmits` payload of [`EMITS_OWNER`], and the emits projection
    /// its value is read from.
    fn emits_payload_keys(
        _: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let events = dispatch
            .graph_arc()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let payload = SemanticQueryKey::ResolveMacroPayload {
            owner: verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                Arc::<str>::from(EMITS_OWNER),
                verter_type_expr::TopLevelOwnerId::instance(0),
                Arc::<str>::from("<sfc-script-setup>"),
            ),
            macro_index: 0,
            macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind::DefineEmits,
            type_args: Arc::from([events]),
            context: verter_type_engine::semantic_query::MacroPayloadContext::new(
                Default::default(),
                ProjectionMode::Expanded,
            ),
        };
        let projection = SemanticQueryKey::ProjectPath {
            base: events,
            path: verter_type_expr::OwnedSlice::from(
                Vec::<verter_type_engine::semantic_query::PathSegment>::new().into_boxed_slice(),
            ),
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        };
        (payload, projection)
    }

    /// `string & ("a" & string) & string & ("a" & "a")`, and its later
    /// subgroup's reduction: the reduction reads its first subgroup, then
    /// the later one after taking the operand between them.
    fn later_subgroup_keys(
        _: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        use verter_type_engine::semantic_query::{
            IntersectionInputRef, IntersectionPurpose, IntersectionTerm, LiteralValue,
            SemanticContextId,
        };
        let graph = dispatch.graph_arc();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let literal =
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("a".into())));
        let first = IntersectionInputRef::from_operands(graph, &[literal, string]);
        let later = IntersectionInputRef::from_operands(graph, &[literal, literal]);
        let purpose = IntersectionPurpose::CheckerReduction;
        let reduction = SemanticQueryKey::ReduceIntersection {
            input: IntersectionInputRef::from_steps(
                graph,
                &[
                    IntersectionTerm::Value(string),
                    IntersectionTerm::EvaluateSubgroup {
                        input: first,
                        purpose,
                    },
                    IntersectionTerm::Value(string),
                    IntersectionTerm::EvaluateSubgroup {
                        input: later.clone(),
                        purpose,
                    },
                ],
            ),
            purpose,
            context: SemanticContextId::production(),
        };
        let nested = SemanticQueryKey::ReduceIntersection {
            input: later,
            purpose,
            context: SemanticContextId::production(),
        };
        (reduction, nested)
    }

    /// The overload set of `pick`'s call signatures, and the result its
    /// composite candidate's node form reads once the list is read. The
    /// list is read here, in place, so its composite's read can be named.
    fn composite_result_keys(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let (set, list) = union_overload_set_keys(host, dispatch);
        let SemanticQueryKey::SignaturesOfType { subject, kind, .. } = list else {
            unreachable!("the set reads a signature list")
        };
        let composite = dispatch
            .first_composite_result_read_for_tests(subject, kind)
            .expect("`pick`'s list synthesizes a composite candidate");
        (set, composite)
    }

    /// The apparent type of `o`'s object surface, and the construct-signature
    /// list its strict-interface choice reads after the call list.
    fn apparent_construct_list_keys(
        _: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let (apparent, list) = apparent_type_keys(dispatch);
        let SemanticQueryKey::SignaturesOfType {
            subject, context, ..
        } = list
        else {
            unreachable!("the anchor walk reads a signature list")
        };
        let constructs = SemanticQueryKey::SignaturesOfType {
            subject,
            kind: verter_type_engine::semantic_query::SignatureKind::Construct,
            context,
        };
        (apparent, constructs)
    }

    /// The apparent type of `o`'s object surface, and the instantiation of
    /// the global `Function` interface it widens to.
    fn apparent_interface_keys(
        _: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        let (apparent, _) = apparent_type_keys(dispatch);
        let interface = dispatch
            .apparent_interface_read_for_tests(CONTEXTUAL_FILE, "Function")
            .expect("the file declares the global `Function`");
        (apparent, interface)
    }

    fn cycle_hop_host() -> Arc<crate::VerterHost> {
        let host = chain_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            TASK_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(
                "type Root = { child: Child }; type Child = { value: string };",
            ),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(TASK_FILE)
            .expect("indexed declarations");
        host
    }

    fn cycle_hop_keys(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        use verter_type_engine::semantic_query::{DeclIdentity, InstantiateKey};
        let identity = |name: &str| DeclIdentity {
            canonical_id: TASK_FILE.into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: host.ensure_indexed_ready(TASK_FILE).unwrap().whole_hash,
            decl_name: name.into(),
        };
        let child = identity("Child");
        (
            dispatch.materialization_cycle_gate_key_for(&identity("Root")),
            SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(child.canonical_id, child.owner, child.decl_name),
                Arc::from([]),
                dispatch.instantiate_context_for(
                    TASK_FILE,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Skeleton,
                    ),
                ),
            )),
        )
    }

    fn routed_declaration_host() -> Arc<crate::VerterHost> {
        let host = unloaded_files_host(&[
            (UNLOADED_OWNER, "export interface Props { a: string }"),
            (
                "/semantic-task/decl-importer.ts",
                "export * from './owner';",
            ),
        ]);
        host.ensure_indexed_ready("/semantic-task/decl-importer.ts")
            .unwrap();
        host.ensure_indexed_ready(UNLOADED_OWNER).unwrap();
        host
    }

    /// One suspension seam of a family built as a continuation: the root a
    /// consumer reads, the dependency another task holds at that seam, and
    /// the stages — builds begun, stages resumed with a read — the root runs
    /// before the seam and in all.
    struct HeldSeam {
        seam: &'static str,
        host: fn() -> Arc<crate::VerterHost>,
        keys: fn(&crate::VerterHost, &HostDispatch<'_>) -> (SemanticQueryKey, SemanticQueryKey),
        /// Whether the dependency is also held at the site an expression
        /// claims it, where the root's need may claim it.
        expression_site: bool,
        before_seam: (u64, u64),
        in_all: (u64, u64),
    }

    /// Every suspension seam of the families built as continuations.
    const HELD_SEAMS: [HeldSeam; 13] = [
        HeldSeam {
            seam: "a routed declaration's defining declaration",
            host: routed_declaration_host,
            keys: |_, _| {
                (
                    declaration_key("/semantic-task/decl-importer.ts"),
                    declaration_key(UNLOADED_OWNER),
                )
            },
            expression_site: false,
            // The scope is read, its wildcard specifier resolved and the
            // owner read as a hop before the route waits on the defining
            // declaration.
            before_seam: (1, 3),
            // The defining lookup begins its own build and reads its scope;
            // the route resumes once with it.
            in_all: (2, 5),
        },
        HeldSeam {
            seam: "a macro payload's props intersection",
            host: chain_host,
            keys: |_, dispatch| props_payload_keys(dispatch),
            expression_site: false,
            // The owner is read, then the payload waits on its intersection.
            before_seam: (1, 1),
            // The intersection begins its own build; the payload resumes
            // once with it.
            in_all: (2, 2),
        },
        HeldSeam {
            seam: "a macro payload's emits projection",
            host: emits_owner_host,
            keys: emits_payload_keys,
            expression_site: false,
            before_seam: (1, 1),
            // The projection is no continuation: only the payload's stages.
            in_all: (1, 2),
        },
        HeldSeam {
            seam: "an intersection's first subgroup",
            host: chain_host,
            keys: |_, dispatch| stepped_intersection_keys(dispatch),
            expression_site: false,
            before_seam: (1, 0),
            in_all: (2, 1),
        },
        HeldSeam {
            seam: "an intersection's later subgroup",
            host: chain_host,
            keys: later_subgroup_keys,
            expression_site: false,
            // The first subgroup is reduced and taken before the wait.
            before_seam: (2, 1),
            in_all: (3, 2),
        },
        HeldSeam {
            seam: "an overload set's signature list",
            host: contextual_host,
            keys: |host, dispatch| {
                let (set, reads) = overload_set_keys(host, dispatch);
                (set, reads[0].clone())
            },
            expression_site: true,
            before_seam: (1, 0),
            in_all: (1, 1),
        },
        HeldSeam {
            seam: "an overload set's composite result",
            host: union_callee_host,
            keys: composite_result_keys,
            expression_site: true,
            // The list is read before the set waits on its composite.
            before_seam: (1, 1),
            in_all: (1, 2),
        },
        HeldSeam {
            seam: "a cycle classification's later declaration hop",
            host: cycle_hop_host,
            keys: cycle_hop_keys,
            expression_site: true,
            before_seam: (1, 2),
            in_all: (1, 3),
        },
        HeldSeam {
            seam: "an apparent type's anchor list",
            host: apparent_callee_host,
            keys: |_, dispatch| apparent_type_keys(dispatch),
            expression_site: true,
            before_seam: (1, 0),
            // The anchor list, the call and construct lists, the
            // `CallableFunction` lookup's library and pending-contributor
            // index reads and its load of the callee file (its `declare
            // global` block is still pending ingestion on a fresh host) and
            // population read, the `Function` lookup's three index reads and
            // its contributor's load, and the interface.
            in_all: (1, 12),
        },
        HeldSeam {
            seam: "an apparent type's construct list",
            host: apparent_callee_host,
            keys: apparent_construct_list_keys,
            expression_site: true,
            // The anchor list and the call list are read before the wait.
            before_seam: (1, 2),
            in_all: (1, 12),
        },
        HeldSeam {
            seam: "an apparent type's interface instantiation",
            host: apparent_callee_host,
            keys: apparent_interface_keys,
            expression_site: true,
            // Every list and both lookups are made before the wait. The
            // keys' own interface read ingested the pending contributor
            // already, so the `CallableFunction` lookup loads none.
            before_seam: (1, 10),
            in_all: (1, 11),
        },
        HeldSeam {
            seam: "a class instance surface's instantiation",
            host: instance_class_host,
            keys: class_instance_keys,
            expression_site: true,
            // The declaring file is read before the wait.
            before_seam: (1, 1),
            in_all: (1, 2),
        },
        HeldSeam {
            seam: "a template product's union reduction",
            host: chain_host,
            keys: |_, dispatch| {
                use verter_type_engine::semantic_query::LiteralValue;
                let graph = dispatch.graph_arc();
                let literal = |text: &str| {
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(text.into())))
                };
                let a = literal("a");
                let b = literal("b");
                let args = graph.intern_node(SemanticNodeData::Union(
                    verter_type_engine::semantic_query::composite::CompositeList::test_fixture(
                        Arc::from([a, b]),
                    ),
                ));
                (
                    SemanticQueryKey::TemplateLiteralReduce {
                        pattern: verter_type_expr::OwnedSlice::from(vec![verter_type_expr::OwnedStr::from("x"), verter_type_expr::OwnedStr::from("")]),
                        args: Arc::from([args]),
                        context: dispatch.template_literal_reduce_context(),
                    },
                    SemanticQueryKey::ReduceUnion {
                        members: verter_type_engine::semantic_query::stable_key::canonicalize_union_members(graph, &[literal("xa"), literal("xb")]),
                        nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
                    },
                )
            },
            expression_site: true,
            before_seam: (1, 0),
            in_all: (1, 1),
        },
    ];

    /// What a root observed while another task held its dependency at a
    /// seam, and what it read once released.
    struct HeldRun {
        yielded: bool,
        frames: Option<usize>,
        pending_elsewhere: Option<bool>,
        held_stages: (u64, u64),
        woken: bool,
        stages: (u64, u64),
        read: ValueRead,
    }

    /// The tasks holding producers, and the producers they hold.
    type HeldProducers<'g> = (
        Vec<verter_execution::tasks::ExecutionTask>,
        Vec<verter_type_engine::semantic_query_memo::ProducerLease<'g>>,
    );

    /// The producers of `held` another task holds: at the site a frame's
    /// need claims it and, for `expression_site`, at an expression's.
    fn hold_seam<'g>(
        host: &crate::VerterHost,
        graph: &'g verter_type_engine::semantic_query_memo::SemanticGraphStore,
        held: SemanticQueryKey,
        expression_site: bool,
    ) -> HeldProducers<'g> {
        let mut holds = vec![hold_producer(host, graph, held.clone())];
        if expression_site {
            holds.push(hold_producer_with(
                host,
                graph,
                held,
                ReadCapture::expression_for_tests(),
            ));
        }
        holds.into_iter().unzip()
    }

    /// The stages `host` runs while `run` runs.
    fn stages_of<R>(host: &crate::VerterHost, run: impl FnOnce() -> R) -> (R, (u64, u64)) {
        let before = continuation_stages(host);
        let out = run();
        let after = continuation_stages(host);
        (out, (after.0 - before.0, after.1 - before.1))
    }

    /// `seam`'s root polled at the production entry on a fresh host while
    /// another task holds its dependency: the first poll, its task polled on
    /// another thread while the producer holds, then — released — the task
    /// resumed on another thread to its answer, delivered here.
    fn poll_across(seam: &HeldSeam) -> (Arc<crate::VerterHost>, HeldRun) {
        let host = (seam.host)();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (root, held) = (seam.keys)(&host, &dispatch);
        let (tasks, leases) = hold_seam(&host, graph, held, seam.expression_site);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(root);
        let (first, frames, pending_elsewhere, held_stages) = while_held(leases, || {
            let first = entry.poll(&waker);
            let held_stages = continuation_stages(&host);
            let task = entry.task();
            let frames = task.as_ref().map(|task| task.live_frames_for_tests());
            let pending_elsewhere = task.map(|task| {
                assert_send(task);
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| task.poll(&waker).is_pending())
                        .join()
                        .expect("the pending poll returns")
                })
            });
            (first, frames, pending_elsewhere, held_stages)
        });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => resume_elsewhere(&mut entry),
        };
        drop(entry);
        let after = continuation_stages(&host);
        let run = HeldRun {
            yielded,
            frames,
            pending_elsewhere,
            held_stages: (held_stages.0 - before.0, held_stages.1 - before.1),
            woken,
            stages: (after.0 - before.0, after.1 - before.1),
            read,
        };
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "{}",
            seam.seam
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "{}", seam.seam);
        drop(tasks);
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    /// `seam`'s root read through the synchronous entry on a fresh host: its
    /// summary and the stages it ran.
    fn synchronous_across(seam: &HeldSeam) -> (ReadSummary, (u64, u64)) {
        let host = (seam.host)();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (root, _) = (seam.keys)(&host, &dispatch);
        let (read, stages) = stages_of(&host, || dispatch.execute_via_cold_build_helper(root));
        (read_summary(&dispatch, &read), stages)
    }

    /// At every suspension seam of a family built as a continuation, a root
    /// whose dependency another task holds returns `Pending` at the polled
    /// entry having run exactly the stages before the seam, its `Send` task
    /// staying pending on another thread while the producer holds. Released,
    /// the task resumes on another thread at the stage it retained — no
    /// stage before the seam runs again — and the root delivers exactly what
    /// the synchronous entry, which drives the same continuation in place,
    /// reads on a fresh host: answer, fact status, rails, diagnostics, the
    /// whole receipt tree and the dependencies its value was computed from.
    /// Warm, the polled and synchronous entries answer alike without
    /// running a stage.
    ///
    /// The stage counts are absolute, so a stage either driver ran again
    /// fails here even when both drivers ran it again alike.
    #[test]
    fn every_converted_seam_suspends_and_resumes_elsewhere_to_the_synchronous_read() {
        for seam in &HELD_SEAMS {
            assert_seam_suspends_and_resumes(seam);
        }
    }

    /// `seam`'s root on fresh hosts: it yields at its held dependency having
    /// run exactly the stages before the seam, waits in one frame, stays
    /// pending on another thread, resumes there at the stage it retained to
    /// what the synchronous entry reads, and warm runs no stage.
    fn assert_seam_suspends_and_resumes(seam: &HeldSeam) {
        let (synchronous, synchronous_stages) = synchronous_across(seam);
        assert_eq!(
            synchronous_stages, seam.in_all,
            "{}: the synchronous entry runs each stage once",
            seam.seam
        );
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "{}: premise: a complete, priced answer: {synchronous:?}",
            seam.seam
        );

        let (host, run) = poll_across(seam);
        assert!(run.yielded, "{}: the root yields at the seam", seam.seam);
        assert_eq!(
            run.held_stages, seam.before_seam,
            "{}: the stages before the seam ran once and the build waits",
            seam.seam
        );
        assert_eq!(run.frames, Some(1), "{}: the root's frame waits", seam.seam);
        assert_eq!(
            run.pending_elsewhere,
            Some(true),
            "{}: its task stays pending on another thread while held",
            seam.seam
        );
        assert!(run.woken, "{}: the release wakes the root", seam.seam);
        assert_eq!(
            run.stages, seam.in_all,
            "{}: no stage runs again across the wait",
            seam.seam
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(
            read_summary(&dispatch, &run.read),
            synchronous,
            "{}",
            seam.seam
        );

        let mut dispatch = dispatch;
        let _scope = dispatch.enter_construction_scope();
        let (root, _) = (seam.keys)(&host, &dispatch);
        let (warm, stages) = stages_of(&host, || {
            let mut entry = dispatch.poll_query(root.clone());
            match entry.poll(Waker::noop()) {
                Poll::Ready(read) => read,
                Poll::Pending => panic!("{}: a warm root answers on its first poll", seam.seam),
            }
        });
        let warm_sync = dispatch.execute_via_cold_build_helper(root);
        assert_eq!(
            read_summary(&dispatch, &warm),
            read_summary(&dispatch, &warm_sync),
            "{}",
            seam.seam
        );
        assert_eq!(stages, (0, 0), "{}: a warm root runs no stage", seam.seam);
    }

    /// At every suspension seam, a root released while it waits — dropped
    /// by its consumer, or stopped by its request's cancellation, which wakes
    /// it with the producer still held — gives its dispatcher back with
    /// nothing open, no wait and no flight but the holding producers', and
    /// the dispatcher then reads the root as the synchronous entry does.
    #[test]
    fn a_root_released_or_cancelled_at_any_converted_seam_leaves_nothing_open() {
        for seam in &HELD_SEAMS {
            assert_seam_releases(seam);
        }
    }

    /// `seam`'s root released while it waits — dropped, or stopped by its
    /// request's cancellation — leaves nothing open, and the dispatcher then
    /// reads it as the synchronous entry does.
    fn assert_seam_releases(seam: &HeldSeam) {
        let (synchronous, _) = synchronous_across(seam);
        for cancelled in [false, true] {
            let host = (seam.host)();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let graph = host.project_type_store().semantic_graph();
            let request = budgeted_request(1);
            let (root, held) = if cancelled {
                let _request = verter_type_engine::request_context::RequestContextGuard::install(
                    Arc::clone(&request),
                );
                (seam.keys)(&host, &dispatch)
            } else {
                (seam.keys)(&host, &dispatch)
            };
            let (tasks, leases) = hold_seam(&host, graph, held, seam.expression_site);
            let counter = Arc::new(CountingWaker::default());
            let waker = Waker::from(Arc::clone(&counter));
            let readers = graph.signature_store().live_reader_count();
            let (pending, stopped, flights) = while_held(leases, || {
                if cancelled {
                    let _request =
                        verter_type_engine::request_context::RequestContextGuard::install(
                            Arc::clone(&request),
                        );
                    dispatch
                        .with_semantic_task_for_tests(root.clone(), |slot| {
                            let task = slot.as_mut().expect("the task is lent");
                            let pending = task.poll_for_tests(&waker).is_pending();
                            let before = counter.0.load(std::sync::atomic::Ordering::SeqCst);
                            request.cancel();
                            let woken =
                                counter.0.load(std::sync::atomic::Ordering::SeqCst) > before;
                            let stopped = woken
                                && matches!(
                                    task.poll_for_tests(&waker),
                                    Poll::Ready(TaskOutcomeForTests::Stopped)
                                )
                                && task.live_frames_for_tests() == 0;
                            (
                                pending,
                                stopped,
                                graph.resident_flight_keys_for_tests().len(),
                            )
                        })
                        .expect("the cold root runs as a task")
                } else {
                    let mut entry = dispatch.poll_query(root.clone());
                    let pending = entry.poll(&waker).is_pending();
                    drop(entry);
                    (pending, true, graph.resident_flight_keys_for_tests().len())
                }
            });
            let label = format!("{}, cancelled={cancelled}", seam.seam);
            assert!(pending, "{label}: the root yields at the seam");
            assert!(
                stopped,
                "{label}: the cancellation wakes the waiting root, which stops"
            );
            assert_eq!(
                flights, 1,
                "{label}: only the holders' flight of the held dependency remains"
            );
            assert_eq!(
                dispatch.open_request_state_for_tests(),
                NOTHING_OPEN,
                "{label}"
            );
            assert_eq!(
                graph.signature_store().live_reader_count(),
                readers,
                "{label}: no signature epoch stays pinned for the released root"
            );
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "{label}");
            drop(tasks);
            let read = dispatch.execute_via_cold_build_helper(root);
            assert_eq!(
                decision(read_summary(&dispatch, &read)),
                decision(synchronous.clone()),
                "{label}"
            );
        }
    }

    /// An overload set whose signature list the kernel retires while another
    /// task holds the result its composite candidate reads: once released,
    /// that read — named in the retired epoch — is refused, and the set,
    /// its first stage never run again, names its composite's read once more
    /// after one pass copies the composite into the current epoch. It never
    /// reads its list again. Retired a second time while the rebound read is
    /// held, the set names it again from the epoch it was copied into. Each
    /// way it answers exactly what the synchronous entry answers with no
    /// retirement, leaving nothing open.
    ///
    /// Reading the list once more instead walks the callee again; answering
    /// the rebound read in place instead waits on the held producer.
    #[test]
    fn an_overload_set_whose_list_retires_while_its_composite_result_is_held_rebinds_the_composite_and_reads_its_list_once(
    ) {
        let seam = &HELD_SEAMS[6];
        let (synchronous, _) = synchronous_across(seam);

        // One retirement. The work counts from before the keys, whose list
        // read walks `pick`'s union and its two arms and compares the arms'
        // one parameter position once each way; the set's own list read is
        // warm.
        {
            let host = (seam.host)();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let graph = host.project_type_store().semantic_graph();
            let before = signature_work(&host);
            let (set, composite) = (seam.keys)(&host, &dispatch);
            let (tasks, leases) = hold_seam(&host, graph, composite, seam.expression_site);
            let mut entry = dispatch.poll_query(set);
            let (read, stages) = stages_of(&host, || {
                let pending = while_held(leases, || {
                    let pending = entry.poll(Waker::noop()).is_pending();
                    graph
                        .signature_store()
                        .replace_epoch()
                        .expect("the kernel replaces its epoch");
                    pending
                });
                assert!(
                    pending,
                    "the set yields while its composite's result is held"
                );
                resume_elsewhere(&mut entry)
            });
            drop(entry);
            assert_eq!(
                stages,
                (1, 3),
                "the set begins once and reads its list, the refused read of its retired composite and the result under the rebound descriptor, each once"
            );
            assert_eq!(
                signature_work(&host).since(before),
                SignatureWork {
                    discovery: 3,
                    positions: 2,
                    forced: 1,
                    rebinds: 1,
                    descriptors: 3,
                },
                "the callee is walked once, the composite copied once and its result forced once"
            );
            assert_eq!(
                decision(read_summary(&dispatch, &read)),
                decision(synchronous.clone())
            );
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            drop(tasks);
        }

        // Two retirements. The work counts from after the first key; the
        // second key's in-place list read walks the union again in the
        // current epoch, the memo never serving the retired list.
        {
            let host = (seam.host)();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let graph = host.project_type_store().semantic_graph();
            let (set, first) = (seam.keys)(&host, &dispatch);
            let SemanticQueryKey::ResolveOverloadSet { callee, kind, .. } = &set else {
                unreachable!("the seam's root is an overload set")
            };
            let (callee, kind) = (*callee, *kind);
            let before = signature_work(&host);
            let before_stages = continuation_stages(&host);
            let (first_tasks, first_leases) =
                hold_seam(&host, graph, first.clone(), seam.expression_site);
            let mut entry = dispatch.poll_query(set);
            let (second_tasks, second_leases) = while_held(first_leases, || {
                assert!(
                    entry.poll(Waker::noop()).is_pending(),
                    "the set yields while its composite's result is held"
                );
                graph
                    .signature_store()
                    .replace_epoch()
                    .expect("the kernel replaces its epoch");
                // The entry holds its dispatcher; another dispatcher of the
                // same host names the read in place.
                let second = {
                    let namer = ProjectSemanticDispatch::new(&*host);
                    let _scope = namer.enter_construction_scope();
                    namer
                        .first_composite_result_read_for_tests(callee, kind)
                        .expect("the current epoch names the composite's read")
                };
                let SemanticQueryKey::ReadSignatureResult(named) = &second else {
                    panic!("a composite waits on its result: {second:?}")
                };
                assert_eq!(
                    named.descriptor.epoch(),
                    graph.signature_store().epoch(),
                    "premise: the held read is named in the current epoch"
                );
                assert_ne!(second, first, "premise: the retired read is another key");
                hold_seam(&host, graph, second, seam.expression_site)
            });
            let (pending, held_stages) = while_held(second_leases, || {
                let pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(Waker::noop()).is_pending()
                };
                let held_stages = continuation_stages(&host);
                graph
                    .signature_store()
                    .replace_epoch()
                    .expect("the kernel replaces its epoch again");
                (pending, held_stages)
            });
            assert!(
                pending,
                "the set waits on the held read its rebound composite names, without waiting it out"
            );
            assert_eq!(
                (
                    held_stages.0 - before_stages.0,
                    held_stages.1 - before_stages.1
                ),
                (1, 2),
                "the list and the refused read, each once"
            );
            let read = resume_elsewhere(&mut entry);
            drop(entry);
            let after_stages = continuation_stages(&host);
            assert_eq!(
                (
                    after_stages.0 - before_stages.0,
                    after_stages.1 - before_stages.1
                ),
                (1, 4),
                "the list, both refused reads and the result under the twice rebound descriptor, each once"
            );
            assert_eq!(
                signature_work(&host).since(before),
                SignatureWork {
                    discovery: 3,
                    positions: 2,
                    forced: 1,
                    rebinds: 2,
                    descriptors: 6,
                },
                "the set copies its composite once per retirement and forces its result once"
            );
            assert_eq!(
                decision(read_summary(&dispatch, &read)),
                decision(synchronous)
            );
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            drop(second_tasks);
            drop(first_tasks);
        }
    }

    /// A class declared in a file the workspace holds, unloaded.
    const UNLOADED_CLASS: &str = "/semantic-task/class.ts";

    /// A class's instance surface whose declaring file is still loading —
    /// its scheduler source read held — returns `Pending` at the polled entry
    /// with its first stage retained, instead of driving or waiting out the
    /// load; its `Send` task stays pending on another thread. Once the read
    /// goes on, the load completes and wakes the root, whose task resumes on
    /// another thread, reads the file once and instantiates the class, and
    /// delivers exactly what the synchronous entry, which loads the file in
    /// place, reads on a fresh host — submitting the same loads.
    #[test]
    fn a_class_instance_surface_whose_declaring_file_is_loading_resumes_elsewhere_once_loaded() {
        let loads_begun = |host: &crate::VerterHost| {
            host.test_force
                .loads_begun
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        let (synchronous, synchronous_loads) = {
            let host = unloaded_files_host(&[(UNLOADED_CLASS, INSTANCE_CLASS)]);
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let surface = class_instance_surface_in(&host, &dispatch, UNLOADED_CLASS);
            let before = loads_begun(&host);
            let (read, stages) =
                stages_of(&host, || dispatch.execute_via_cold_build_helper(surface));
            assert_eq!(
                stages,
                (1, 2),
                "the surface begins once and reads its file and its instantiation once"
            );
            (read_summary(&dispatch, &read), loads_begun(&host) - before)
        };
        assert!(
            synchronous.0.contains("marker") && !synchronous.3,
            "premise: the loaded class's complete instance type: {synchronous:?}"
        );

        let host = unloaded_files_host(&[(UNLOADED_CLASS, INSTANCE_CLASS)]);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let surface = class_instance_surface_in(&host, &dispatch, UNLOADED_CLASS);
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(UNLOADED_CLASS);
        let held = HeldSource(&gate);
        let before = (continuation_stages(&host), loads_begun(&host));
        let mut entry = dispatch.poll_query(surface);
        let counter = CountingThreadWaker::for_current_thread();
        let counting = Waker::from(Arc::clone(&counter));
        let first = entry.poll(&counting).is_pending();
        let held_stages = continuation_stages(&host);
        let loaded_while_held = host.scheduler.try_get_source(UNLOADED_CLASS).is_some();
        let elsewhere = entry.task().map(|task| {
            assert_send(task);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| task.poll(&counting).is_pending())
                    .join()
                    .expect("the pending poll returns")
            })
        });
        drop(held);
        assert!(first, "the surface yields while its file loads");
        assert_eq!(
            (held_stages.0 - before.0 .0, held_stages.1 - before.0 .1),
            (1, 0),
            "the first stage ran once and waits on the load"
        );
        assert!(!loaded_while_held, "the file's source read is still held");
        assert_eq!(elsewhere, Some(true));
        counter.wait_for_wake("the completed load wakes the waiting poll");
        let read = resume_elsewhere(&mut entry);
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0 .0, after.1 - before.0 .1),
            (1, 2),
            "no stage runs again: the file is read once after its load"
        );
        assert_eq!(
            loads_begun(&host) - before.1,
            synchronous_loads,
            "the polled entry submits the loads the synchronous entry submits"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    /// A family waiting on its file's load — the file's scheduler source
    /// read held — released by its consumer, or stopped by its request's
    /// cancellation, which wakes it with the read still held, withdraws only
    /// its own wait: nothing stays open, the load it began goes on, and the
    /// dispatcher then reads the family exactly as the synchronous entry does
    /// on a fresh host.
    #[test]
    fn a_family_released_or_cancelled_while_its_file_loads_leaves_nothing_open() {
        type Key = fn(&crate::VerterHost, &HostDispatch<'_>) -> SemanticQueryKey;
        let cases: [(&str, &str, &str, Key); 2] = [
            (
                "a macro payload's owner",
                UNLOADED_OWNER,
                "export interface Props { a: string }\n",
                |_, dispatch| owned_props_payload_keys(dispatch, UNLOADED_OWNER).0,
            ),
            (
                "a class's declaring file",
                UNLOADED_CLASS,
                INSTANCE_CLASS,
                |host, dispatch| class_instance_surface_in(host, dispatch, UNLOADED_CLASS),
            ),
        ];
        for (case, file, source, key) in cases {
            let synchronous = {
                let host = unloaded_files_host(&[(file, source)]);
                let dispatch = ProjectSemanticDispatch::new(&*host);
                let root = key(&host, &dispatch);
                read_summary(&dispatch, &dispatch.execute_via_cold_build_helper(root))
            };
            for cancelled in [false, true] {
                let label = format!("{case}, cancelled={cancelled}");
                let host = unloaded_files_host(&[(file, source)]);
                let mut dispatch = ProjectSemanticDispatch::new(&*host);
                let graph = host.project_type_store().semantic_graph();
                let root = key(&host, &dispatch);
                let gate = Arc::clone(&host.test_force.source_load_gate);
                gate.hold(file);
                let held = HeldSource(&gate);
                let counter = Arc::new(CountingWaker::default());
                let waker = Waker::from(Arc::clone(&counter));
                let (pending, stopped) = if cancelled {
                    let request = budgeted_request(1);
                    let _request =
                        verter_type_engine::request_context::RequestContextGuard::install(
                            Arc::clone(&request),
                        );
                    dispatch
                        .with_semantic_task_for_tests(root.clone(), |slot| {
                            let task = slot.as_mut().expect("the task is lent");
                            let pending = task.poll_for_tests(&waker).is_pending();
                            let before = counter.0.load(std::sync::atomic::Ordering::SeqCst);
                            request.cancel();
                            let woken =
                                counter.0.load(std::sync::atomic::Ordering::SeqCst) > before;
                            let stopped = woken
                                && matches!(
                                    task.poll_for_tests(&waker),
                                    Poll::Ready(TaskOutcomeForTests::Stopped)
                                )
                                && task.live_frames_for_tests() == 0;
                            (pending, stopped)
                        })
                        .expect("the cold root runs as a task")
                } else {
                    let mut entry = dispatch.poll_query(root.clone());
                    let pending = entry.poll(&waker).is_pending();
                    drop(entry);
                    (pending, true)
                };
                let open = dispatch.open_request_state_for_tests();
                let waits = graph.wait_graph_counts_for_tests().1;
                drop(held);
                assert!(pending, "{label}: the family yields while its file loads");
                assert!(
                    stopped,
                    "{label}: the cancellation wakes the waiting family, which stops"
                );
                assert_eq!(open, NOTHING_OPEN, "{label}");
                assert_eq!(waits, 0, "{label}: the released family holds no wait");
                let read = dispatch.execute_via_cold_build_helper(root);
                assert_eq!(read_summary(&dispatch, &read), synchronous, "{label}");
                assert!(graph.resident_flight_keys_for_tests().is_empty(), "{label}");
            }
        }
    }

    /// A barrel one hop past the scope's own imports.
    const DEEP_IMPORTER: &str = "/semantic-task/deep-importer.ts";
    const DEEP_BARREL: &str = "/semantic-task/deep-barrel.ts";

    /// A host whose declaration lookup in [`DEEP_IMPORTER`] reaches
    /// [`UNLOADED_OWNER`] through two barrels: the importer's `export *`
    /// and the deeper barrel's named re-export. Both barrels are indexed;
    /// the owner is loaded when `owner_indexed`, else left unloaded.
    fn deep_barrel_host(owner_indexed: bool) -> Arc<crate::VerterHost> {
        let host = unloaded_files_host(&[
            (DEEP_IMPORTER, "export * from './deep-barrel';"),
            (DEEP_BARREL, "export { Props } from './owner';"),
            (UNLOADED_OWNER, "export interface Props { a: string }"),
        ]);
        host.ensure_indexed_ready(DEEP_IMPORTER).unwrap();
        host.ensure_indexed_ready(DEEP_BARREL).unwrap();
        if owner_indexed {
            host.ensure_indexed_ready(UNLOADED_OWNER).unwrap();
        }
        host
    }

    /// A declaration lookup follows its scope's export route hop by hop,
    /// each barrel a load of its own: a defining file two barrels away,
    /// whose index another shard still builds, returns `Pending` at the
    /// polled entry with the route retained — the scope, the first barrel
    /// and both specifier resolutions each read once — without entering a
    /// wait, and its `Send` task stays pending on another thread. Released,
    /// the lookup resumes at the deeper hop, reads the defining declaration
    /// once and delivers exactly what the synchronous entry, which drives
    /// the same route in place, reads on a fresh host, with the same
    /// stages. No file the route read ahead of its hops: nothing of the
    /// deeper barrel's target is loaded before the lookup stands at it.
    #[test]
    fn a_routed_declaration_whose_deeper_barrel_target_is_indexing_yields_and_resumes_elsewhere() {
        let (synchronous, synchronous_stages) = {
            let host = deep_barrel_host(false);
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (read, stages) = stages_of(&host, || {
                dispatch.execute_via_cold_build_helper(declaration_key(DEEP_IMPORTER))
            });
            (read_summary(&dispatch, &read), stages)
        };
        assert_eq!(
            synchronous_stages,
            (2, 7),
            "the lookup begins once and resumes at its scope, both specifiers and both hops; the defining lookup begins once and resumes at its scope, then the route resumes with it"
        );
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );

        let host = deep_barrel_host(false);
        assert!(host.ensure_loaded(UNLOADED_OWNER));
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(declaration_key(DEEP_IMPORTER));
        let counter = CountingThreadWaker::for_current_thread();
        let waker = Waker::from(Arc::clone(&counter));
        let first = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let held_stages = continuation_stages(&host);
        let elsewhere = entry.task().map(|task| {
            assert_send(task);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| task.poll(&waker).is_pending())
                    .join()
                    .expect("the pending poll returns")
            })
        });
        held.release();
        assert!(
            first,
            "the lookup yields while the deeper target's index builds"
        );
        assert_eq!(
            (held_stages.0 - before.0, held_stages.1 - before.1),
            (1, 4),
            "the scope, the first barrel and both specifiers were read once and the route waits at the deeper target"
        );
        assert_eq!(elsewhere, Some(true));
        counter.wait_for_wake("the completed index wakes the waiting lookup");
        let read = resume_elsewhere(&mut entry);
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            synchronous_stages,
            "no stage runs again: the route resumes at the deeper target"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());

        // Warm, the polled entry answers alike on its first poll without
        // running a stage.
        let (warm, stages) = {
            let mut warm = dispatch.poll_query(declaration_key(DEEP_IMPORTER));
            stages_of(&host, || match warm.poll(Waker::noop()) {
                Poll::Ready(read) => read,
                Poll::Pending => panic!("a warm lookup answers at once"),
            })
        };
        assert_eq!(stages, (0, 0), "a warm lookup runs no stage");
        assert_eq!(read_summary(&dispatch, &warm), synchronous);
    }

    /// The module whose `declare global` block contributes the `Function`
    /// interface a callable's apparent type widens to.
    const FUNCTION_CONTRIBUTOR: &str = "/semantic-task/function-global.ts";

    const FUNCTION_CONTRIBUTION: &str =
        "declare global { interface Function { readonly marker: \"fn\"; } }\n";

    /// A callable object surface in the contextual file, whose project
    /// declares `Function` only through `contribution`, the module source
    /// of [`FUNCTION_CONTRIBUTOR`]; both files indexed.
    fn contributed_function_host(contribution: &str) -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            FUNCTION_CONTRIBUTOR,
            &crate::u6_flow_shape_corpus_tests::module_script(contribution),
            crate::FileLanguage::script_ts(),
        );
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            CONTEXTUAL_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(
                "declare const o: { (a: string): 1 };\n",
            ),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(FUNCTION_CONTRIBUTOR)
            .expect("the contributor has indexed input");
        host.ensure_indexed_ready(CONTEXTUAL_FILE)
            .expect("the callee has indexed input");
        host
    }

    /// The `Function` contribution after an edit: the same interface, a
    /// comment added, so the module's index must be built again.
    const EDITED_FUNCTION_CONTRIBUTION: &str =
        "// edited\ndeclare global { interface Function { readonly marker: \"fn\"; } }\n";

    /// A host whose `Function` contributor was indexed and then edited: its
    /// source is current, its index cold and the contributor pending
    /// ingestion again, so the next global lookup reads it as a load.
    fn edited_function_contributor_host() -> Arc<crate::VerterHost> {
        let host = contributed_function_host(FUNCTION_CONTRIBUTION);
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            FUNCTION_CONTRIBUTOR,
            &crate::u6_flow_shape_corpus_tests::module_script(EDITED_FUNCTION_CONTRIBUTION),
            crate::FileLanguage::script_ts(),
        );
        host
    }

    /// A global declaration lookup reads the program contributors it would
    /// otherwise index in place as loads of its own: an apparent type whose
    /// `Function` interface a `declare global` module contributes, that
    /// module's index cold after an edit and its shard held, returns
    /// `Pending` at the polled entry at the contributor's load — only the
    /// stages before it run — without entering a wait, and its `Send` task
    /// stays pending on another thread. Released, it resumes at the
    /// contributor and delivers exactly what the synchronous entry reads
    /// on an equally edited host, with the same stages: no stage runs
    /// again, and no load is made ahead of the lookup's own.
    #[test]
    fn an_apparent_type_whose_global_contributor_is_indexing_yields_and_resumes_elsewhere() {
        let (synchronous, synchronous_stages) = {
            let host = edited_function_contributor_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let (apparent, _) = apparent_type_keys(&dispatch);
            let (read, stages) =
                stages_of(&host, || dispatch.execute_via_cold_build_helper(apparent));
            (read_summary(&dispatch, &read), stages)
        };
        assert!(
            synchronous.0.contains("marker") && !synchronous.3,
            "premise: the contributed interface's complete surface: {synchronous:?}"
        );

        let host = edited_function_contributor_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (apparent, _) = apparent_type_keys(&dispatch);
        // The held shards keep the edited contributor's index cold until
        // released.
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let before = continuation_stages(&host);
        let mut entry = dispatch.poll_query(apparent.clone());
        let counter = CountingThreadWaker::for_current_thread();
        let waker = Waker::from(Arc::clone(&counter));
        let first = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let held_stages = continuation_stages(&host);
        let elsewhere = entry.task().map(|task| {
            assert_send(task);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| task.poll(&waker).is_pending())
                    .join()
                    .expect("the pending poll returns")
            })
        });
        held.release();
        assert!(
            first,
            "the apparent type yields while its contributor's index builds"
        );
        let held_stages = (held_stages.0 - before.0, held_stages.1 - before.1);
        assert_eq!(held_stages.0, 1, "only the apparent type's build began");
        assert!(
            held_stages.1 >= 4 && held_stages.1 < synchronous_stages.1,
            "the lists and the lookup's index reads ran and the lookup waits at its contributor: {held_stages:?} of {synchronous_stages:?}"
        );
        assert_eq!(elsewhere, Some(true));
        counter.wait_for_wake("the completed index wakes the waiting lookup");
        let read = resume_elsewhere(&mut entry);
        drop(entry);
        let after = continuation_stages(&host);
        assert_eq!(
            (after.0 - before.0, after.1 - before.1),
            synchronous_stages,
            "no stage runs again: the lookup resumes at its contributor"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    /// The static surface of the class `C` declared in `file`.
    fn class_static_surface_in(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        file: &str,
    ) -> SemanticQueryKey {
        let env = host.host_view_env_hashes_for(file);
        SemanticQueryKey::ResolveClassSurface {
            decl_slot: dispatch.type_slot_for(
                file,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "C",
            ),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            side: verter_type_engine::semantic_query::ClassSurfaceSide::Static,
            context: verter_type_engine::semantic_query::ClassSurfaceContext {
                parse_env_hash: env.parse_env_hash,
                resolve_env_hash: env.resolve_env_hash,
                mode: ProjectionMode::Shallow,
            },
        }
    }

    /// A file declaring a value of an interface type, unloaded.
    const UNLOADED_VALUE: &str = "/semantic-task/value.ts";

    const UNLOADED_VALUE_SOURCE: &str = "export const value = { a: \"x\" };\n";

    /// A loaded file whose value is typed by an interface it imports from
    /// [`UNLOADED_VALUE`].
    const IMPORTING_FILE: &str = "/semantic-task/importer.ts";

    const IMPORTING_SOURCE: &str =
        "import { value } from \"./value\";\nexport const copy = value;\n";

    /// The instantiation of the `Props` [`UNLOADED_OWNER`] declares.
    fn unloaded_props_key(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
            dispatch.type_slot_for(
                UNLOADED_OWNER,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "Props",
            ),
            Arc::from([]),
            dispatch.instantiate_context_for(
                UNLOADED_OWNER,
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            ),
        ))
    }

    /// The `typeof` of the value `name` that `canonical` declares.
    fn file_typeof_key(
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        canonical: &str,
        name: &str,
    ) -> SemanticQueryKey {
        dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    canonical,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: verter_type_expr::OwnedStr::from(name),
            },
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
    }

    /// The instantiations `host` built.
    fn instantiations(host: &crate::VerterHost) -> u64 {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .instantiate_count
    }

    /// The load contract of a frame's dependency on a file still loading.
    /// The synchronous entry loads `held` in place. The polled entry, with
    /// the scheduler's source read of `held` held, returns `Pending` instead
    /// of driving or waiting out the load, leaves the file unloaded, and its
    /// `Send` task stays pending on another thread. Once the read goes on,
    /// the completed load wakes the root, which delivers exactly what the
    /// synchronous entry reads on a fresh host — answer, fact status,
    /// diagnostics and receipt, logical work included — having built each
    /// instantiation as often as the synchronous entry did, with nothing
    /// left open.
    fn assert_frame_waits_on_a_loading_file(
        host: impl Fn() -> Arc<crate::VerterHost>,
        held: &str,
        key: impl Fn(
            &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        ) -> SemanticQueryKey,
        answer: &str,
    ) {
        let synchronous = {
            let host = host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(key(&dispatch));
            assert!(
                host.scheduler.try_get_source(held).is_some(),
                "the synchronous entry loads {held} in place"
            );
            (read_summary(&dispatch, &read), instantiations(&host))
        };
        assert_eq!(synchronous.0 .0, answer, "{synchronous:?}");
        assert!(
            !synchronous.0 .3 && synchronous.0 .5.is_some(),
            "the read is complete and priced: {synchronous:?}"
        );

        let host = host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(held);
        let guard = HeldSource(&gate);
        let mut entry = dispatch.poll_query(key(&dispatch));
        let counter = CountingThreadWaker::for_current_thread();
        let counting = Waker::from(Arc::clone(&counter));
        let first = entry.poll(&counting).is_pending();
        let loaded_while_held = host.scheduler.try_get_source(held).is_some();
        let elsewhere = entry.task().map(|task| {
            assert_send(task);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| task.poll(&counting).is_pending())
                    .join()
                    .expect("the pending poll returns")
            })
        });
        drop(guard);
        assert!(first, "the root yields while {held} loads");
        assert!(
            !loaded_while_held,
            "the source read of {held} is still held"
        );
        assert_eq!(elsewhere, Some(true), "its task stays pending elsewhere");
        counter.wait_for_wake("the completed load wakes the waiting poll");
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        assert_eq!(
            (read_summary(&dispatch, &read), instantiations(&host)),
            synchronous,
            "the polled root reads what the synchronous entry reads, building nothing again"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    /// An instantiation whose declaring file is still loading waits on the
    /// load before its build begins.
    #[test]
    fn an_instantiation_whose_declaring_file_is_loading_yields_and_reads_it_once_loaded() {
        assert_frame_waits_on_a_loading_file(
            unloaded_owner_host,
            UNLOADED_OWNER,
            unloaded_props_key,
            "{ a: string }",
        );
    }

    /// A `typeof` whose value's file is still loading waits on the load
    /// before its build begins.
    #[test]
    fn a_typeof_whose_value_file_is_loading_yields_and_reads_it_once_loaded() {
        assert_frame_waits_on_a_loading_file(
            || unloaded_files_host(&[(UNLOADED_VALUE, UNLOADED_VALUE_SOURCE)]),
            UNLOADED_VALUE,
            |dispatch| file_typeof_key(dispatch, UNLOADED_VALUE, "value"),
            "{ a: string }",
        );
    }

    /// A `typeof` in a loaded file whose initializer names a value imported
    /// from a file still loading: before its build begins, the importing
    /// frame waits on the load its file's prepared declarations route that
    /// import to, instead of the bundle loading it in place.
    #[test]
    fn a_typeof_reading_an_import_from_a_loading_file_yields_and_reads_it_once_loaded() {
        let host = || {
            let host = unloaded_files_host(&[
                (UNLOADED_VALUE, UNLOADED_VALUE_SOURCE),
                (IMPORTING_FILE, IMPORTING_SOURCE),
            ]);
            assert!(host.ensure_loaded(IMPORTING_FILE), "the importer is loaded");
            assert!(
                host.scheduler.try_get_source(UNLOADED_VALUE).is_none(),
                "loading the importer leaves its import unloaded"
            );
            host
        };
        assert_frame_waits_on_a_loading_file(
            host,
            UNLOADED_VALUE,
            |dispatch| file_typeof_key(dispatch, IMPORTING_FILE, "copy"),
            "{ a: string }",
        );
    }

    /// An instantiation released while its declaring file loads withdraws
    /// only its own wait: nothing stays open, the load goes on for the next
    /// demand, and the request then reads what the synchronous entry reads.
    #[test]
    fn an_instantiation_released_while_its_declaring_file_loads_leaves_nothing_open() {
        let synchronous = {
            let host = unloaded_owner_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let read = dispatch.execute_via_cold_build_helper(unloaded_props_key(&dispatch));
            read_summary(&dispatch, &read)
        };
        let host = unloaded_owner_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let gate = Arc::clone(&host.test_force.source_load_gate);
        gate.hold(UNLOADED_OWNER);
        let guard = HeldSource(&gate);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let mut entry = dispatch.poll_query(unloaded_props_key(&dispatch));
        let first = entry.poll(&waker).is_pending();
        drop(entry);
        let open = dispatch.open_request_state_for_tests();
        let waits = graph.wait_graph_counts_for_tests().1;
        let flights = graph.resident_flight_keys_for_tests();
        drop(guard);
        assert!(first, "the instantiation yields while its file loads");
        assert_eq!(open, NOTHING_OPEN);
        assert_eq!(waits, 0, "the released root holds no wait");
        assert!(flights.is_empty(), "the released root leaves no flight");
        let read = dispatch.execute_via_cold_build_helper(unloaded_props_key(&dispatch));
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// A macro payload released while it waits on its payload operation
    /// withdraws its wait and releases its frame, leaving only the other
    /// producer's flight; the request then answers the payload.
    #[test]
    fn a_macro_payload_released_while_it_waits_leaves_nothing_open() {
        let host = chain_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let (payload, intersection) = props_payload_keys(&dispatch);
        let (other, lease) = hold_producer(&host, graph, intersection.clone());
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let mut entry = dispatch.poll_query(payload.clone());
        let (first, released) = while_held(lease, || {
            let first = entry.poll(&waker).is_pending();
            drop(entry);
            (
                first,
                (
                    graph.wait_graph_counts_for_tests().1,
                    graph.resident_flight_keys_for_tests(),
                ),
            )
        });
        assert!(first, "the payload yields while its intersection is held");
        assert_eq!(
            released,
            (0, vec![intersection]),
            "the released payload withdraws its wait and leaves only the other producer's flight"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(payload);
        assert!(
            matches!(
                read.value(),
                QueryResult::Value(SemanticQueryValue::TypeNode(_))
            ),
            "the request goes on: {read:?}"
        );
    }

    /// A call root uses the same owned candidate walk as a nested call. A
    /// parameter's held alias must yield before the relation consumes it.
    #[test]
    fn a_call_root_suspends_at_an_operand_dependency_and_delivers_its_call_value() {
        use verter_type_engine::semantic_query::{
            CallKind, DeclIdentity, InstantiateKey, LiteralValue, PayloadArray,
        };
        let keys = |host: &crate::VerterHost,
                    dispatch: &ProjectSemanticDispatch<
            '_,
            crate::resolver_core::HostCapabilities,
        >| {
            let callee = dispatch.execute_read(dispatch.typeof_key_for(
                ValueRootKey {
                    scope: ScopeId::file(
                        CALL_FILE,
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    ),
                    name: "pick".into(),
                },
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            ));
            let QueryResult::Value(callee) = callee.value() else {
                panic!("the declared function has a callable type");
            };
            let graph = dispatch.graph_arc();
            let one = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(1.0)));
            let two = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(2.0)));
            let boxed = graph.intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: CALL_FILE.into(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: host.shallow_file_state(CALL_FILE).unwrap().whole_hash,
                    decl_name: "Box".into(),
                },
                args: PayloadArray::from(vec![two]),
            });
            let call = super::super::call_resolve_tests::call_key_at(
                dispatch,
                CALL_FILE,
                *callee,
                CallKind::Call,
                None,
                vec![
                    super::super::call_resolve_tests::eager(one),
                    super::super::call_resolve_tests::eager(boxed),
                ],
            );
            let held = SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(
                    CALL_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    "Box",
                ),
                Arc::from([two]),
                dispatch.instantiate_context_for(
                    CALL_FILE,
                    ProjectionReductionContext::structural_transit(),
                ),
            ));
            (SemanticQueryKey::ResolveCall(Box::new(call)), held)
        };
        let synchronous = {
            let host = call_host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let _builder = dispatch.enter_construction_scope();
            let (key, _) = keys(&host, &dispatch);
            let read = dispatch.execute_via_cold_build_helper(key);
            assert!(
                matches!(
                    read.value(),
                    QueryResult::Value(SemanticQueryValue::ResolveCall(result))
                        if matches!(result.as_ref(), verter_type_engine::semantic_query::ResolvedCallResult::Selected { .. })
                ),
                "the declared generic call selects: {:?}",
                read.value()
            );
            read_summary(&dispatch, &read)
        };
        let host = call_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _builder = dispatch.enter_construction_scope();
        let (key, held) = keys(&host, &dispatch);
        let graph = host.project_type_store().semantic_graph();
        let (other, lease) = hold_producer(&host, graph, held);
        let waker = Waker::from(Arc::new(CountingWaker::default()));
        let mut entry = dispatch.poll_query(key);
        let first = while_held(lease, || entry.poll(&waker).is_pending());
        assert!(
            first,
            "the call yields while its operand dependency is held"
        );
        let Poll::Ready(read) = entry.poll(&waker) else {
            panic!("the call finishes once its operand is available");
        };
        drop(entry);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
    }

    /// A module of generic aliases a path reads through:
    /// `Pair<"deep">["inner"]["value"]` instantiates `Pair<"deep">`, then
    /// the `Box<"deep">` its `inner` member applies.
    const PROJECTION_FILE: &str = "/semantic-task/projection.ts";

    const PROJECTION: &str = "type Box<T> = { value: T };\n\
                              type Pair<T> = { inner: Box<T> };\n";

    fn projection_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            PROJECTION_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(PROJECTION),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(PROJECTION_FILE)
            .expect("the aliases have indexed input before a hop is held");
        host
    }

    /// The path `Pair<"deep">["inner"]["value"]` read in `Navigate`, and
    /// the instantiations its walk reads, in the order it reads them: its
    /// base `Pair<"deep">`, then the `Box<"deep">` the `inner` member
    /// applies — each an intermediate hop, read in structural transit.
    fn projection_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, [SemanticQueryKey; 2]) {
        use verter_type_engine::semantic_query::{
            DeclIdentity, InstantiateKey, LiteralValue, PathSegment, PayloadArray, PropertyKey,
        };
        let graph = dispatch.graph_arc();
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let deep = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "deep".into(),
        )));
        let pair = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: DeclIdentity {
                canonical_id: PROJECTION_FILE.into(),
                owner,
                whole_hash: host
                    .shallow_file_state(PROJECTION_FILE)
                    .expect("the aliases are indexed")
                    .whole_hash,
                decl_name: "Pair".into(),
            },
            args: PayloadArray::from(vec![deep]),
        });
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let path = SemanticQueryKey::ProjectPath {
            base: pair,
            path: verter_type_expr::OwnedSlice::from(
                vec![
                    PathSegment::Member(PropertyKey::identifier("inner")),
                    PathSegment::Member(PropertyKey::identifier("value")),
                ]
                .into_boxed_slice(),
            ),
            context,
        };
        let hop = |alias: &str| {
            SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(PROJECTION_FILE, owner, alias),
                Arc::from([deep]),
                dispatch.instantiate_context_for(
                    PROJECTION_FILE,
                    context.into_structural_transit_with_mode(ProjectionMode::Navigate),
                ),
            ))
        };
        (path, [hop("Pair"), hop("Box")])
    }

    /// The hops path walks on `host` computed, wherever they ran.
    fn projection_hops(host: &crate::VerterHost) -> u64 {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .projection_hops_walked
    }

    /// The hops `Pair<"deep">["inner"]["value"]` walks: the base
    /// instantiated, `inner` read off it, its `Box<"deep">` instantiated,
    /// `value` read off that.
    const PROJECTION_WALK_HOPS: u64 = 4;

    /// What a polled path did while one of its instantiations was held.
    struct HeldProjection {
        /// The first poll returned `Pending` instead of waiting the
        /// producer out.
        yielded: bool,
        /// Hops computed by the first poll.
        hops_at_hold: u64,
        /// A second poll here while still held — under a probe that
        /// forbids every wait — and then its task polled on another thread
        /// stayed pending.
        repolled_pending: bool,
        pending_elsewhere: Option<bool>,
        /// Hops computed by those repolls.
        repoll_hops: u64,
        /// The producer going away woke the root.
        woken: bool,
        /// Hops computed over the whole polled read.
        hops: u64,
        read: ValueRead,
    }

    /// The path polled at the production entry on a fresh host while
    /// another task produces `projection_keys(..).1[held]`; released, its
    /// task is resumed on another thread and the answer delivered here.
    fn poll_projection_holding(held: usize) -> (Arc<crate::VerterHost>, HeldProjection) {
        let host = projection_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (path, hops) = projection_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, hops[held].clone());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = projection_hops(&host);
        let mut entry = dispatch.poll_query(path);
        let (first, hops_at_hold, repolled_pending, pending_elsewhere, repoll_hops) =
            while_held(lease, || {
                let first = entry.poll(&waker);
                let hops_at_hold = projection_hops(&host) - before;
                if first.is_ready() {
                    return (first, hops_at_hold, false, None, 0);
                }
                let repolled_pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(&waker).is_pending()
                };
                let pending_elsewhere = entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                });
                let repoll_hops = projection_hops(&host) - before - hops_at_hold;
                (
                    first,
                    hops_at_hold,
                    repolled_pending,
                    pending_elsewhere,
                    repoll_hops,
                )
            });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => resume_elsewhere(&mut entry),
        };
        drop(entry);
        let run = HeldProjection {
            yielded,
            hops_at_hold,
            repolled_pending,
            pending_elsewhere,
            repoll_hops,
            woken,
            hops: projection_hops(&host) - before,
            read,
        };
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    /// The path read through the synchronous entry on a fresh host: its
    /// summary and the hops it computed.
    fn synchronous_projection() -> (ReadSummary, u64) {
        let host = projection_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (path, _) = projection_keys(&host, &dispatch);
        let before = projection_hops(&host);
        let read = dispatch.execute_via_cold_build_helper(path);
        (
            read_summary(&dispatch, &read),
            projection_hops(&host) - before,
        )
    }

    /// A cold path projection whose walk reaches an instantiation another
    /// task is producing — its base, the first dependency the walk reads —
    /// returns `Pending` at the polled entry instead of waiting that
    /// producer out, having computed only the hop that reads it. Polled
    /// again while held, under a probe that forbids every wait, and then
    /// polled on another thread, it stays pending and computes nothing.
    /// Released, it resumes on another thread at that hop and delivers
    /// exactly what the synchronous entry reads on a fresh host — answer,
    /// fact status, rails, diagnostics and the whole receipt tree — having
    /// computed each hop once: the absolute count, so a walk both entries
    /// replayed alike still fails.
    #[test]
    fn a_cold_path_whose_internal_instantiation_is_held_yields_and_resumes_to_the_synchronous_read()
    {
        let (synchronous, synchronous_hops) = synchronous_projection();
        assert_eq!(synchronous.0, "\"deep\"", "{synchronous:?}");
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(synchronous_hops, PROJECTION_WALK_HOPS);

        let (host, run) = poll_projection_holding(0);
        assert!(
            run.yielded,
            "the path yields while another task instantiates its base"
        );
        assert_eq!(run.hops_at_hold, 1, "the walk waits at its first hop");
        assert!(
            run.repolled_pending,
            "a repoll while held stays pending without entering a wait"
        );
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_hops, 0, "a pending repoll computes no hop");
        assert!(run.woken, "the producer going away wakes the path");
        assert_eq!(
            run.hops, PROJECTION_WALK_HOPS,
            "no hop is computed again: the walk resumes at the hop it held"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);

        // Warm, both entries answer alike computing no hop.
        let (path, _) = projection_keys(&host, &dispatch);
        let before = projection_hops(&host);
        let mut dispatch = dispatch;
        let mut warm = dispatch.poll_query(path.clone());
        let Poll::Ready(warm_read) = warm.poll(Waker::noop()) else {
            panic!("a warm path answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(path);
        assert_eq!(projection_hops(&host), before, "a warm path walks no hop");
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
    }

    /// Sequential reads: a path whose walk already read its base's
    /// instantiation and the member off it, and now reads the `Box<"deep">`
    /// that member applies while another task produces it, returns
    /// `Pending` holding the hops it completed. Resumed on another thread,
    /// it continues at the held hop — the completed hops and their read
    /// are not computed again, counted as physical hops where they are
    /// computed, outside every charge and its rollback — and delivers
    /// exactly what the synchronous entry reads, receipt tree included.
    #[test]
    fn a_path_waiting_on_its_second_instantiation_resumes_without_walking_its_completed_hops_again()
    {
        let (synchronous, synchronous_hops) = synchronous_projection();
        assert_eq!(synchronous_hops, PROJECTION_WALK_HOPS);

        let (host, run) = poll_projection_holding(1);
        assert!(
            run.yielded,
            "the path yields while another task instantiates its member's alias"
        );
        assert_eq!(
            run.hops_at_hold, 3,
            "the base, its member and the member's alias were walked before the wait"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_hops, 0, "a pending repoll computes no hop");
        assert!(run.woken);
        assert_eq!(
            run.hops, PROJECTION_WALK_HOPS,
            "the completed hops are not walked again: one hop remained after the wait"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A path released while it waits on its held instantiation withdraws
    /// its wait and leaves nothing open; the producer it waited on is left
    /// alone, and the dispatcher reads the path afterwards as the
    /// synchronous entry does.
    #[test]
    fn a_path_released_while_its_instantiation_is_held_leaves_nothing_open() {
        let (synchronous, _) = synchronous_projection();
        let host = projection_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (path, hops) = projection_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, hops[1].clone());
        let mut entry = dispatch.poll_query(path.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(Waker::noop()).is_pending();
            drop(entry);
            pending
        });
        assert!(pending, "the path yields while its instantiation is held");
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(path);
        assert_eq!(read_summary(&dispatch, &read).0, synchronous.0);
    }

    /// The admission steps query entries computed on `host`, wherever they
    /// ran: one per stage of an entry's sugar, carrier, apparent-wrapper and
    /// signature-subject normalization.
    fn admission_stages(host: &crate::VerterHost) -> u64 {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .admission_stages_walked
    }

    /// The steps deferred evaluations computed on `host`, wherever they
    /// ran: one per evaluator step, counted where the step runs, outside
    /// every charge and its rollback.
    fn deferred_steps(host: &crate::VerterHost) -> u64 {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .deferred_steps_evaluated
    }

    /// The physical work a held read is measured by: path hops, admission
    /// stages and evaluator steps, and the candidates explicit overload sets
    /// began instantiating, the substitutions their clauses began and the
    /// children substitution walks visited.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Work {
        hops: u64,
        admission: u64,
        deferred: u64,
        candidates: u64,
        binders: u64,
        visits: u64,
    }

    impl Work {
        /// The work done since `before`.
        fn minus(self, before: Work) -> Work {
            Work {
                hops: self.hops - before.hops,
                admission: self.admission - before.admission,
                deferred: self.deferred - before.deferred,
                candidates: self.candidates - before.candidates,
                binders: self.binders - before.binders,
                visits: self.visits - before.visits,
            }
        }
    }

    fn work(host: &crate::VerterHost) -> Work {
        let stats = host.project_type_store().semantic_graph().stats_snapshot();
        Work {
            hops: projection_hops(host),
            admission: admission_stages(host),
            deferred: deferred_steps(host),
            candidates: stats.explicit_candidates_instantiated,
            binders: stats.binder_substitutions,
            visits: stats.substitution_child_visits,
        }
    }

    fn work_since(host: &crate::VerterHost, before: Work) -> Work {
        work(host).minus(before)
    }

    const NO_WORK: Work = Work {
        hops: 0,
        admission: 0,
        deferred: 0,
        candidates: 0,
        binders: 0,
        visits: 0,
    };

    /// What a polled root did while one of its dependencies was held.
    struct HeldRead {
        /// The first poll returned `Pending` instead of waiting the
        /// producer out.
        yielded: bool,
        /// Work computed by the first poll.
        at_hold: Work,
        /// A second poll here while still held — under a probe that
        /// forbids every wait — stayed pending, and so did its task polled
        /// on another thread when it has one.
        repolled_pending: bool,
        pending_elsewhere: Option<bool>,
        /// Work computed by those repolls.
        repoll: Work,
        /// The producer going away woke the root.
        woken: bool,
        /// Work computed over the whole polled read.
        total: Work,
        read: ValueRead,
        /// The synchronous waits the first poll entered, recorded: a
        /// dependency joined in place instead of named as the root's need
        /// shows here as a flight or drive wait.
        first_waits: Vec<verter_execution::blocking::WaitFamily>,
    }

    /// `root` polled at the production entry on a fresh projection host
    /// while another task produces `held`; released, a task still pending
    /// is resumed on another thread and the answer delivered here. Nothing
    /// the read held stays open.
    fn poll_root_holding(
        keys: impl FnOnce(&crate::VerterHost, &HostDispatch<'_>) -> (SemanticQueryKey, SemanticQueryKey),
    ) -> (Arc<crate::VerterHost>, HeldRead) {
        poll_root_holding_on(projection_host, keys)
    }

    /// The same, on a fresh host `host` makes.
    fn poll_root_holding_on(
        host: fn() -> Arc<crate::VerterHost>,
        keys: impl FnOnce(&crate::VerterHost, &HostDispatch<'_>) -> (SemanticQueryKey, SemanticQueryKey),
    ) -> (Arc<crate::VerterHost>, HeldRead) {
        poll_root_holding_at(host, keys, false)
    }

    /// The same, the held read also held at the site an expression claims
    /// it when `expression_site`.
    fn poll_root_holding_at(
        host: fn() -> Arc<crate::VerterHost>,
        keys: impl FnOnce(&crate::VerterHost, &HostDispatch<'_>) -> (SemanticQueryKey, SemanticQueryKey),
        expression_site: bool,
    ) -> (Arc<crate::VerterHost>, HeldRead) {
        let host = host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (root, held) = keys(&host, &dispatch);
        let (other, lease) = hold_seam(&host, graph, held, expression_site);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = work(&host);
        let mut entry = dispatch.poll_query(root);
        let (first, at_hold, repolled_pending, pending_elsewhere, repoll, first_waits) =
            while_held(lease, || {
                let (first, first_waits) = {
                    let probe = verter_execution::blocking::WaitProbe::record();
                    let first = entry.poll(&waker);
                    (first, probe.reached())
                };
                let at_hold = work_since(&host, before);
                if first.is_ready() {
                    return (first, at_hold, false, None, NO_WORK, first_waits);
                }
                let repolled_pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(&waker).is_pending()
                };
                let pending_elsewhere = entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                });
                let total = work_since(&host, before);
                let repoll = total.minus(at_hold);
                (
                    first,
                    at_hold,
                    repolled_pending,
                    pending_elsewhere,
                    repoll,
                    first_waits,
                )
            });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => match entry.task() {
                Some(_) => resume_elsewhere(&mut entry),
                None => poll_until_ready(|waker| entry.poll(waker)),
            },
        };
        drop(entry);
        let run = HeldRead {
            yielded,
            at_hold,
            repolled_pending,
            pending_elsewhere,
            repoll,
            woken,
            total: work_since(&host, before),
            read,
            first_waits,
        };
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    /// `root` read through the synchronous entry on a fresh projection
    /// host: its summary and the work it computed.
    fn synchronous_root(
        keys: impl FnOnce(&crate::VerterHost, &HostDispatch<'_>) -> SemanticQueryKey,
    ) -> (ReadSummary, Work) {
        synchronous_root_on(projection_host, keys)
    }

    /// The same, on a fresh host `host` makes.
    fn synchronous_root_on(
        host: fn() -> Arc<crate::VerterHost>,
        keys: impl FnOnce(&crate::VerterHost, &HostDispatch<'_>) -> SemanticQueryKey,
    ) -> (ReadSummary, Work) {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let root = keys(&host, &dispatch);
        let before = work(&host);
        let read = dispatch.execute_via_cold_build_helper(root);
        (read_summary(&dispatch, &read), work_since(&host, before))
    }

    /// `Pair<"deep">` as its deferred `["inner"]` access: a node the path
    /// walker resolves by reading the `IndexedAccess` sugar of its object.
    fn deferred_inner_access(
        dispatch: &HostDispatch<'_>,
        object: verter_type_engine::semantic_query::SemanticNodeId,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        dispatch
            .graph_arc()
            .intern_node(SemanticNodeData::IndexedAccess {
                object,
                index: verter_type_engine::semantic_query::IndexKey::from("inner"),
                origin: verter_type_engine::semantic_query::IndexedAccessOrigin::Composed,
            })
    }

    /// `Pair<"deep">["inner"]["value"]` with its first access deferred:
    /// the path `["value"]` over `Pair<"deep">["inner"]`, whose walk reads
    /// the `IndexedAccess` sugar `Pair<"deep">["inner"]` before its own hop.
    fn sugar_path(host: &crate::VerterHost, dispatch: &HostDispatch<'_>) -> SemanticQueryKey {
        let (path, _) = projection_keys(host, dispatch);
        let SemanticQueryKey::ProjectPath { base, context, .. } = path else {
            unreachable!("the projection is a path")
        };
        SemanticQueryKey::ProjectPath {
            base: deferred_inner_access(dispatch, base),
            path: verter_type_expr::OwnedSlice::from(
                vec![verter_type_engine::semantic_query::PathSegment::Member(
                    verter_type_engine::semantic_query::PropertyKey::identifier("value"),
                )]
                .into_boxed_slice(),
            ),
            context,
        }
    }

    /// The hops `Pair<"deep">["inner"]["value"]` walks with its first
    /// access deferred: the deferred access the outer walk stands at, the
    /// sugar's own walk of `Pair<"deep">` and its `inner` member, then the
    /// outer walk's `Box<"deep">` and its `value` member.
    const SUGAR_WALK_HOPS: u64 = 5;

    /// A path whose walk reads the `IndexedAccess` sugar of a deferred
    /// access — a projection read whose entry canonicalizes its key — while
    /// another task instantiates the sugar's object returns `Pending` at the
    /// polled entry: the sugar is admitted as the canonical path in the
    /// needing frame and becomes that frame's need, instead of being read in
    /// place, where its walk would wait the producer out. Repolled while
    /// held it computes nothing; released, it resumes on another thread and
    /// delivers exactly what the synchronous entry reads, receipt tree
    /// included, having walked each hop once.
    #[test]
    fn a_hop_reading_index_sugar_whose_instantiation_is_held_yields_and_resumes_to_the_synchronous_read(
    ) {
        let (synchronous, synchronous_work) = synchronous_root(sugar_path);
        assert_eq!(synchronous.0, "\"deep\"", "{synchronous:?}");
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(synchronous_work.hops, SUGAR_WALK_HOPS);

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (_, hops) = projection_keys(host, dispatch);
            (sugar_path(host, dispatch), hops[0].clone())
        });
        assert!(
            run.yielded,
            "the path yields while another task instantiates its sugar's object"
        );
        assert_eq!(
            run.at_hold.hops, 2,
            "the outer hop at the deferred access and the sugar's hop at its object ran before the wait"
        );
        assert!(run.repolled_pending, "a repoll while held enters no wait");
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken, "the producer going away wakes the path");
        assert_eq!(
            run.total.hops, SUGAR_WALK_HOPS,
            "no hop is walked again: the sugar's walk resumes at its held hop"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// `Pair` named by an unresolved authored reference: a `BareRef`
    /// carrier over `"deep"`, scoped to the aliases' file.
    fn pair_carrier(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        let graph = dispatch.graph_arc();
        let deep = graph.intern_node(SemanticNodeData::Literal(
            verter_type_engine::semantic_query::LiteralValue::String("deep".into()),
        ));
        let scope = verter_type_engine::semantic_query::NodeScopeId::File {
            canonical_id: PROJECTION_FILE.into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: host
                .shallow_file_state(PROJECTION_FILE)
                .expect("the aliases are indexed")
                .whole_hash,
            local_scope: None,
        };
        graph.intern_node_with_scope(
            SemanticNodeData::new_bare_ref(Arc::from("Pair"), scope.clone(), Arc::from([deep])),
            scope,
        )
    }

    /// The path `Pair<"deep">["inner"]["value"]` over the carrier, read in
    /// `mode`, and the reads its entry's carrier normalization makes in
    /// `Expanded`, in order: the anchor declaration `Pair`, then its
    /// instantiation over `"deep"` under the path's own context.
    fn carrier_path(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        mode: ProjectionMode,
    ) -> (SemanticQueryKey, [SemanticQueryKey; 2]) {
        use verter_type_engine::semantic_query::{
            InstantiateKey, LiteralValue, PathSegment, PropertyKey, ResolveDeclKey,
        };
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let context = ProjectionReductionContext::published(mode);
        let path = SemanticQueryKey::ProjectPath {
            base: pair_carrier(host, dispatch),
            path: verter_type_expr::OwnedSlice::from(
                vec![
                    PathSegment::Member(PropertyKey::identifier("inner")),
                    PathSegment::Member(PropertyKey::identifier("value")),
                ]
                .into_boxed_slice(),
            ),
            context,
        };
        let deep =
            dispatch
                .graph_arc()
                .intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    "deep".into(),
                )));
        let anchor = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId::file(PROJECTION_FILE, owner),
            name: "Pair".into(),
        });
        let instantiation = SemanticQueryKey::Instantiate(InstantiateKey::new(
            dispatch.type_slot_for(PROJECTION_FILE, owner, "Pair"),
            Arc::from([deep]),
            dispatch.instantiate_context_for(PROJECTION_FILE, context),
        ));
        (path, [anchor, instantiation])
    }

    /// The stages an `Expanded` carrier-rooted path's entry computes before
    /// its claim: the carrier planned up to its anchor read, the anchor's
    /// answer taken up to the instantiation read, the instantiation's answer
    /// finishing the admission.
    const CARRIER_ADMISSION_STAGES: u64 = 3;

    fn expanded_carrier_path(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> SemanticQueryKey {
        carrier_path(host, dispatch, ProjectionMode::Expanded).0
    }

    /// A path whose subject is an unresolved reference normalizes that
    /// carrier at its entry, before its claim. Polled while another task
    /// produces the carrier's anchor declaration, the entry returns
    /// `Pending` at that read — its admission retained, not blocked on the
    /// producer — computes nothing when repolled, and released, completes
    /// its admission and its walk, delivering exactly what the synchronous
    /// entry reads, receipt tree included, with each admission stage
    /// computed once.
    #[test]
    fn a_carrier_rooted_path_whose_anchor_declaration_is_held_yields_at_its_admission() {
        let (synchronous, synchronous_work) = synchronous_root(expanded_carrier_path);
        assert_eq!(synchronous.0, "\"deep\"", "{synchronous:?}");
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(synchronous_work.admission, CARRIER_ADMISSION_STAGES);

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (path, reads) = carrier_path(host, dispatch, ProjectionMode::Expanded);
            (path, reads[0].clone())
        });
        assert!(run.yielded, "the entry yields at its held anchor read");
        assert_eq!(
            run.at_hold.admission, 1,
            "the carrier was planned up to its anchor"
        );
        assert_eq!(run.at_hold.hops, 0, "no hop is walked before admission");
        assert!(run.repolled_pending, "a repoll while held enters no wait");
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken, "the producer going away wakes the entry");
        assert_eq!(
            run.total, synchronous_work,
            "each stage and hop is computed once"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// Sequential reads inside one helper: the carrier's admission already
    /// read its anchor declaration and now reads the instantiation that
    /// anchor routes through, which another task is producing. The entry
    /// returns `Pending` holding the anchor's answer; resumed, it continues
    /// at the held instantiation — the anchor stage is not computed again,
    /// counted where it runs, outside every charge and its rollback — and
    /// delivers exactly what the synchronous entry reads.
    #[test]
    fn a_carrier_admission_waiting_on_its_instantiation_resumes_without_reading_its_anchor_again() {
        let (synchronous, synchronous_work) = synchronous_root(expanded_carrier_path);
        assert_eq!(synchronous_work.admission, CARRIER_ADMISSION_STAGES);

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (path, reads) = carrier_path(host, dispatch, ProjectionMode::Expanded);
            (path, reads[1].clone())
        });
        assert!(
            run.yielded,
            "the entry yields at its held instantiation read"
        );
        assert_eq!(
            run.at_hold.admission, 2,
            "the carrier was planned and its anchor read before the wait"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken);
        assert_eq!(
            run.total, synchronous_work,
            "the completed admission stages are not computed again"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// An entry released while its admission waits withdraws its wait and
    /// leaves nothing open — its carrier's cycle guard included — and the
    /// dispatcher reads the path afterwards as the synchronous entry does.
    #[test]
    fn a_carrier_admission_released_while_its_read_is_held_leaves_nothing_open() {
        let (synchronous, _) = synchronous_root(expanded_carrier_path);
        let host = projection_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (path, reads) = carrier_path(&host, &dispatch, ProjectionMode::Expanded);
        let (other, lease) = hold_producer(&host, graph, reads[1].clone());
        let mut entry = dispatch.poll_query(path.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(Waker::noop()).is_pending();
            drop(entry);
            pending
        });
        assert!(pending, "the entry yields while its admission read is held");
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(path);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// `Pair<"deep">["inner"]["value"]` with the deferred access over the
    /// carrier: the walk reads `IndexedAccess` sugar whose subject is the
    /// unresolved reference, so the needing frame admits a carrier.
    fn carrier_sugar_path(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> SemanticQueryKey {
        let SemanticQueryKey::ProjectPath { path, context, .. } = sugar_path(host, dispatch) else {
            unreachable!("the projection is a path")
        };
        SemanticQueryKey::ProjectPath {
            base: deferred_inner_access(dispatch, pair_carrier(host, dispatch)),
            path,
            context,
        }
    }

    /// A hop reading sugar over a carrier subject admits it in the needing
    /// frame: the carrier normalized, its prelude carried to the canonical
    /// path it needs. While another task instantiates the resolved object,
    /// the path returns `Pending`; released, it delivers exactly what the
    /// synchronous entry reads — answer, facts and the receipt tree the
    /// prelude's publication shapes — with each stage computed once.
    #[test]
    fn a_hop_reading_carrier_sugar_admits_it_in_the_frame_and_reads_what_the_synchronous_entry_reads(
    ) {
        let (synchronous, synchronous_work) = synchronous_root(carrier_sugar_path);
        assert_eq!(synchronous.0, "\"deep\"", "{synchronous:?}");
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert!(
            synchronous_work.admission > 0,
            "premise: the sugar's entry normalizes its carrier"
        );

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (_, hops) = projection_keys(host, dispatch);
            (carrier_sugar_path(host, dispatch), hops[0].clone())
        });
        assert!(
            run.yielded,
            "the path yields while the carrier's object is held"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken);
        assert_eq!(
            run.total, synchronous_work,
            "each stage and hop is computed once"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A module contributing a member to the global `String` interface.
    const STRING_CONTRIBUTOR: &str = "/semantic-task/string-global.ts";

    /// A host whose `String` contributor was indexed and then edited: its
    /// source is current, its index cold and the contributor pending
    /// ingestion again, so the next global lookup of `String` reads it as a
    /// load.
    fn edited_string_contributor_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            STRING_CONTRIBUTOR,
            &crate::u6_flow_shape_corpus_tests::module_script(
                "declare global { interface String { readonly marker: \"str\"; } }\n",
            ),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(STRING_CONTRIBUTOR)
            .expect("the contributor has indexed input");
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            STRING_CONTRIBUTOR,
            &crate::u6_flow_shape_corpus_tests::module_script(
                "// edited\ndeclare global { interface String { readonly marker: \"str\"; } }\n",
            ),
            crate::FileLanguage::script_ts(),
        );
        host
    }

    /// `string["marker"]` read in `Navigate`: a path whose first step reads
    /// the scope-less subject's apparent wrapper, which its entry admits
    /// through the demanding project's `String`.
    fn string_marker_path(dispatch: &HostDispatch<'_>) -> SemanticQueryKey {
        SemanticQueryKey::ProjectPath {
            base: dispatch
                .graph_arc()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
            path: verter_type_expr::OwnedSlice::from(
                vec![verter_type_engine::semantic_query::PathSegment::Member(
                    verter_type_engine::semantic_query::PropertyKey::identifier("marker"),
                )]
                .into_boxed_slice(),
            ),
            context: ProjectionReductionContext::published(ProjectionMode::Navigate),
        }
    }

    /// A polled root whose entry admits its key through an apparent wrapper
    /// reads the wrapper's global lookup files as a frame does: with the
    /// `String` contributor's index cold after an edit and its shard held,
    /// the entry returns `Pending` at the contributor's load — under a probe
    /// that forbids every wait, so it neither loads nor waits in place —
    /// with only the admission stages before it computed and no hop walked.
    /// Released, it resumes at that load and delivers exactly what the
    /// synchronous entry reads on an equally edited host, each stage
    /// computed once.
    #[test]
    fn a_wrapper_admission_whose_global_contributor_is_indexing_yields_at_its_load() {
        let (synchronous, synchronous_work) = {
            let host = edited_string_contributor_host();
            let _request =
                verter_type_engine::request_context::install_test_request_for(STRING_CONTRIBUTOR);
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let before = work(&host);
            let read = dispatch.execute_via_cold_build_helper(string_marker_path(&dispatch));
            (read_summary(&dispatch, &read), work_since(&host, before))
        };
        assert!(
            synchronous.0.contains("str") && !synchronous.3,
            "premise: the contributed member, complete: {synchronous:?}"
        );
        assert!(
            synchronous_work.admission > 1,
            "premise: the entry admits through the wrapper's lookup: {synchronous_work:?}"
        );

        let host = edited_string_contributor_host();
        let _request =
            verter_type_engine::request_context::install_test_request_for(STRING_CONTRIBUTOR);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let path = string_marker_path(&dispatch);
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let before = work(&host);
        let mut entry = dispatch.poll_query(path);
        let counter = CountingThreadWaker::for_current_thread();
        let waker = Waker::from(Arc::clone(&counter));
        let first = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let at_hold = work_since(&host, before);
        let repolled = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let repoll = work_since(&host, before);
        held.release();
        assert!(
            first,
            "the entry yields while its wrapper's contributor indexes"
        );
        assert!(repolled, "a repoll while held enters no wait");
        assert_eq!(repoll, at_hold, "a pending repoll computes nothing");
        assert_eq!(at_hold.hops, 0, "no hop is walked before admission");
        assert!(
            at_hold.admission >= 1 && at_hold.admission < synchronous_work.admission,
            "the admission waits inside its lookup: {at_hold:?} of {synchronous_work:?}"
        );
        counter.wait_for_wake("the completed index wakes the waiting admission");
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        assert_eq!(
            work_since(&host, before),
            synchronous_work,
            "no stage runs again: the admission resumes at its load"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
    }

    /// A polled root whose admission waits on its wrapper lookup's load is
    /// woken by its request's cancellation with the load still held — no
    /// timer, no release — and its next poll delivers the terminal
    /// cancelled read, releasing the admission and leaving nothing open.
    #[test]
    fn a_wrapper_admission_waiting_on_its_load_is_woken_and_stopped_by_its_cancellation() {
        let host = edited_string_contributor_host();
        let request =
            verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
                1,
                Arc::from(STRING_CONTRIBUTOR),
                verter_audit::RequestKind::ComponentMeta,
                false,
                false,
                None,
                1_000_000,
            );
        let _request =
            verter_type_engine::request_context::RequestContextGuard::install(Arc::clone(&request));
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let graph = host.project_type_store().semantic_graph();
        let path = string_marker_path(&dispatch);
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        let mut entry = dispatch.poll_query(path);
        // Each poll runs under a probe that forbids every wait, so an
        // in-place wait fails at once instead of waiting the held shards out.
        let first = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let before = wakes();
        std::thread::scope(|scope| {
            scope
                .spawn(|| request.cancel())
                .join()
                .expect("the request is cancelled on another thread");
        });
        let woken = wakes() > before;
        let next = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            match entry.poll(&waker) {
                Poll::Ready(read) => Some(read),
                Poll::Pending => None,
            }
        };
        drop(entry);
        held.release();
        assert!(
            first,
            "the entry yields while its wrapper's contributor indexes"
        );
        assert!(
            woken,
            "the request's cancellation wakes the waiting admission"
        );
        let read = next.expect("the woken admission reads its cancellation with the load held");
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Cancelled)
            ),
            "the stopped admission reads a terminal cancellation: {read:?}"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    }

    const RELATION_FILE: &str = "/semantic-task/relation.ts";

    /// An alias whose body applies another: relating two of its
    /// applications instantiates each operand before the pair relates.
    const RELATION: &str = "type Box<T> = { v: T };\n\
                            type Wrap<T> = { w: Box<T> };\n";

    fn relation_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            RELATION_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(RELATION),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(RELATION_FILE)
            .expect("the aliases have indexed input before an operand is held");
        host
    }

    /// Aliases of `Wrap<"a">` and `Wrap<string>` related as a machinery
    /// root, and the instantiations its operands' carrier walks read, in
    /// the order they read them: the source's `Wrap<"a">`, then the
    /// target's `Wrap<string>`, each in structural transit.
    fn relation_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    ) -> (SemanticQueryKey, [SemanticQueryKey; 2]) {
        use verter_type_engine::semantic_query::{
            DeclIdentity, InstantiateKey, LiteralValue, PayloadArray,
        };
        let graph = dispatch.graph_arc();
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("a".into())));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let whole_hash = host
            .shallow_file_state(RELATION_FILE)
            .expect("the aliases are indexed")
            .whole_hash;
        let wrap = |arg: verter_type_engine::semantic_query::SemanticNodeId| {
            graph.intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: RELATION_FILE.into(),
                    owner,
                    whole_hash,
                    decl_name: "Wrap".into(),
                },
                args: PayloadArray::from(vec![arg]),
            })
        };
        // Aliases of the applications: nothing before the relation's
        // structural dispatch reads through them, so the dispatch's operand
        // unwraps are the relation's first reads.
        let alias = |node| graph.intern_node(SemanticNodeData::Alias(node));
        let relate = dispatch
            .relate_key_for(alias(wrap(a)), alias(wrap(string)))
            .to_query_key();
        let operand = |arg: verter_type_engine::semantic_query::SemanticNodeId| {
            SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(RELATION_FILE, owner, "Wrap"),
                Arc::from([arg]),
                dispatch.instantiate_context_for(
                    RELATION_FILE,
                    ProjectionReductionContext::structural_transit(),
                ),
            ))
        };
        (relate, [operand(a), operand(string)])
    }

    /// The physical work relations on `host` did, wherever they ran: the
    /// machinery roots whose root obligation frame opened, the structural
    /// reductions, and the instantiations built. Each is counted where the
    /// work is done, outside every charge and its rollback.
    fn relation_work(host: &crate::VerterHost) -> [u64; 3] {
        let stats = host.project_type_store().semantic_graph().stats_snapshot();
        [
            stats.relation_roots_opened,
            stats.relation_reductions,
            stats.instantiate_count,
        ]
    }

    fn relation_work_since(after: [u64; 3], before: [u64; 3]) -> [u64; 3] {
        [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ]
    }

    /// The relation read through the synchronous entry on a fresh host: its
    /// summary and the work it did.
    fn synchronous_relation() -> (ReadSummary, [u64; 3]) {
        let host = relation_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (relate, _) = relation_keys(&host, &dispatch);
        let before = relation_work(&host);
        let read = dispatch.execute_via_cold_build_helper(relate);
        (
            read_summary(&dispatch, &read),
            relation_work_since(relation_work(&host), before),
        )
    }

    /// What a polled relation root did while one of its operands'
    /// instantiations was held.
    struct HeldRelation {
        /// The first poll returned `Pending` instead of waiting the
        /// producer out.
        yielded: bool,
        /// Work done by the first poll.
        at_hold: [u64; 3],
        /// A second poll here while still held — under a probe that
        /// forbids every wait — and then its task polled on another thread
        /// stayed pending.
        repolled_pending: bool,
        pending_elsewhere: Option<bool>,
        /// Work done by those repolls.
        repoll_work: [u64; 3],
        /// The producer going away woke the root.
        woken: bool,
        /// Work done over the whole polled read.
        work: [u64; 3],
        read: ValueRead,
    }

    /// The relation polled at the production entry on a fresh host while
    /// another task produces `relation_keys(..).1[held]`; released, its task
    /// is resumed on another thread and the answer delivered here.
    fn poll_relation_holding(held: usize) -> (Arc<crate::VerterHost>, HeldRelation) {
        let host = relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, operands) = relation_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, operands[held].clone());
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = relation_work(&host);
        let mut entry = dispatch.poll_query(relate);
        let (first, at_hold, repolled_pending, pending_elsewhere, repoll_work) =
            while_held(lease, || {
                let first = entry.poll(&waker);
                let at_hold = relation_work_since(relation_work(&host), before);
                if first.is_ready() {
                    return (first, at_hold, false, None, [0; 3]);
                }
                let repolled_pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(&waker).is_pending()
                };
                let pending_elsewhere = entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                });
                let repoll_work =
                    relation_work_since(relation_work_since(relation_work(&host), before), at_hold);
                (
                    first,
                    at_hold,
                    repolled_pending,
                    pending_elsewhere,
                    repoll_work,
                )
            });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => resume_elsewhere(&mut entry),
        };
        drop(entry);
        let run = HeldRelation {
            yielded,
            at_hold,
            repolled_pending,
            pending_elsewhere,
            repoll_work,
            woken,
            work: relation_work_since(relation_work(&host), before),
            read,
        };
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    /// The synchronous premise of the relation rows: `Wrap<"a">` is
    /// assignable to `Wrap<string>` — a complete, priced answer — and the
    /// relation runs as one root whose reduction relates the instantiated
    /// operands.
    fn relation_premise() -> (ReadSummary, [u64; 3]) {
        let (synchronous, work) = synchronous_relation();
        assert!(
            synchronous.0.contains("Assignable") && !synchronous.0.contains("NotAssignable"),
            "premise: the pair is assignable: {synchronous:?}"
        );
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(work[0], 1, "premise: one machinery root: {work:?}");
        assert!(
            work[1] >= 1 && work[2] >= 2,
            "premise: the root reduces over both instantiated operands: {work:?}"
        );
        (synchronous, work)
    }

    /// A relation root whose source operand's instantiation another task is
    /// producing returns `Pending` at the polled entry with its root
    /// obligation frame open and its reduction standing at its structural
    /// dispatch, whose first reads unwrap the operands, instead of waiting
    /// the producer out. Polled again while held, under a probe that forbids
    /// every wait, and on another thread, it stays pending and does no work.
    /// Released, it resumes on another thread at its operand demand —
    /// opening no second root and reducing once — and delivers exactly what
    /// the synchronous entry reads, receipt tree included, with the same
    /// absolute work.
    #[test]
    fn a_relation_root_whose_source_operand_is_held_yields_with_its_root_frame_open() {
        let (synchronous, synchronous_work) = relation_premise();
        let (host, run) = poll_relation_holding(0);
        assert!(
            run.yielded,
            "the root yields while another task instantiates its source"
        );
        assert_eq!(
            run.at_hold,
            [1, 1, 0],
            "the root's frame is open, its reduction waits at its structural dispatch, nothing is built"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 3], "a pending repoll does no work");
        assert!(run.woken, "the producer going away wakes the root");
        assert_eq!(
            run.work, synchronous_work,
            "no root opens again and no reduction runs twice"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// Sequential reads: a relation root that already instantiated its
    /// source operand, and now waits on its target's instantiation while
    /// another task produces it, returns `Pending` holding its open root
    /// frame and the completed source operand. Resumed on another thread,
    /// it continues at the target — the source is not instantiated again,
    /// no root opens again, the pair reduces once — and delivers exactly
    /// what the synchronous entry reads, receipt tree included. Warm, both
    /// entries answer alike doing no work.
    #[test]
    fn a_relation_root_waiting_on_its_target_operand_resumes_without_repeating_its_source() {
        let (synchronous, synchronous_work) = relation_premise();
        let (host, run) = poll_relation_holding(1);
        assert!(
            run.yielded,
            "the root yields while another task instantiates its target"
        );
        assert_eq!(
            run.at_hold[..2],
            [1, 1],
            "the root's frame is open and its reduction waits at its dispatch while the target is held"
        );
        assert!(
            run.at_hold[2] >= 1,
            "the source operand was instantiated before the wait: {:?}",
            run.at_hold
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 3], "a pending repoll does no work");
        assert!(run.woken);
        assert_eq!(
            run.work, synchronous_work,
            "the completed source operand and the open root are not repeated"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);

        let (relate, _) = relation_keys(&host, &dispatch);
        let before = relation_work(&host);
        let mut dispatch = dispatch;
        let mut warm = dispatch.poll_query(relate.clone());
        let Poll::Ready(warm_read) = warm.poll(Waker::noop()) else {
            panic!("a warm relation answers on its first poll");
        };
        drop(warm);
        let warm_sync = dispatch.execute_via_cold_build_helper(relate);
        assert_eq!(relation_work(&host), before, "a warm relation does no work");
        assert_eq!(
            read_summary(&dispatch, &warm_read),
            read_summary(&dispatch, &warm_sync)
        );
    }

    /// A relation root released while it waits on its held target operand
    /// closes its root obligation frame and withdraws its wait, leaving
    /// nothing open; the producer it waited on is left alone, and the
    /// dispatcher reads the relation afterwards as the synchronous entry
    /// does.
    #[test]
    fn a_relation_root_released_while_its_operand_is_held_leaves_nothing_open() {
        let (synchronous, _) = relation_premise();
        let host = relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, operands) = relation_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, operands[1].clone());
        let mut entry = dispatch.poll_query(relate.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(Waker::noop()).is_pending();
            drop(entry);
            pending
        });
        assert!(pending, "the root yields while its operand is held");
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "the released root closes its obligation frame"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(relate);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// A relation root waiting on its held target operand is woken by its
    /// request's cancellation with the producer still held, and its next
    /// poll delivers the terminal cancelled read, its root frame closed and
    /// its wait withdrawn while the producer it waited on is left alone.
    #[test]
    fn a_relation_root_is_woken_by_its_requests_cancellation_while_its_operand_is_held() {
        let host = relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, operands) = relation_keys(&host, &dispatch);
        let (other, lease) = hold_producer(&host, graph, operands[1].clone());
        let request = budgeted_request(1);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        let (first, woken, next) = {
            let _request = verter_type_engine::request_context::RequestContextGuard::install(
                Arc::clone(&request),
            );
            let mut entry = dispatch.poll_query(relate.clone());
            let observed = while_held(lease, || {
                let first = entry.poll(&waker).is_pending();
                let before = wakes();
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| request.cancel())
                        .join()
                        .expect("the request is cancelled on another thread");
                });
                let woken = wakes() > before;
                let next = match entry.poll(&waker) {
                    Poll::Ready(read) => Some(read),
                    Poll::Pending => None,
                };
                (first, woken, next)
            });
            drop(entry);
            observed
        };
        assert!(first, "the root yields while its operand is held");
        assert!(
            woken,
            "the request's cancellation wakes the root with the producer still held"
        );
        let read = next.expect("the woken root reads its cancellation without the producer");
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Cancelled)
            ),
            "the cancelled root reads a terminal cancellation: {read:?}"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(other);
    }

    const MEMBER_RELATION_FILE: &str = "/semantic-task/member-relation.ts";

    /// Two recursive interfaces whose first members relate back to the
    /// pair itself and whose later members relate two applications of
    /// distinct aliases: relating `Left` to `Right` records the recursive
    /// member's back-edge before the later member instantiates its
    /// operands. `OuterLeft` and `OuterRight` wrap the pair in a member of
    /// their own.
    const MEMBER_RELATION: &str = "type BoxA<T> = { v: T };\n\
                                   type BoxB<T> = { v: T };\n\
                                   interface Left { aNext: Left; zLeaf: BoxA<\"a\"> }\n\
                                   interface Right { aNext: Right; zLeaf: BoxB<string> }\n\
                                   interface OuterLeft { inner: Left }\n\
                                   interface OuterRight { inner: Right }\n";

    fn member_relation_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            MEMBER_RELATION_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(MEMBER_RELATION),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(MEMBER_RELATION_FILE)
            .expect("the interfaces have indexed input before a member is held");
        host
    }

    /// The interface `pair.0` related to `pair.1` as a machinery root, and
    /// the instantiation the later member of `Left` and `Right` reads first:
    /// `BoxA<"a">` in structural transit.
    fn member_relation_keys(
        host: &crate::VerterHost,
        dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        pair: (&str, &str),
    ) -> (SemanticQueryKey, SemanticQueryKey) {
        use verter_type_engine::semantic_query::{
            DeclIdentity, InstantiateKey, LiteralValue, NodeScopeId,
        };
        let graph = dispatch.graph_arc();
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let whole_hash = host
            .shallow_file_state(MEMBER_RELATION_FILE)
            .expect("the interfaces are indexed")
            .whole_hash;
        let declaration = |name: &str| {
            graph.intern_node(SemanticNodeData::DeclRef {
                identity: DeclIdentity {
                    canonical_id: MEMBER_RELATION_FILE.into(),
                    owner,
                    whole_hash,
                    decl_name: name.into(),
                },
            })
        };
        let relate = dispatch
            .relate_key_for(declaration(pair.0), declaration(pair.1))
            .to_query_key();
        // The argument as the file lowered it: a literal bound to its
        // declaration's file, not the scope-less literal a fixture interns.
        let a = graph.intern_node_with_scope(
            SemanticNodeData::Literal(LiteralValue::String("a".into())),
            NodeScopeId::File {
                canonical_id: MEMBER_RELATION_FILE.into(),
                owner,
                whole_hash,
                local_scope: None,
            },
        );
        let leaf = SemanticQueryKey::Instantiate(InstantiateKey::new(
            dispatch.type_slot_for(MEMBER_RELATION_FILE, owner, "BoxA"),
            Arc::from([a]),
            dispatch.instantiate_context_for(
                MEMBER_RELATION_FILE,
                ProjectionReductionContext::structural_transit(),
            ),
        ));
        (relate, leaf)
    }

    /// The physical work relations on `host` did, wherever they ran: the
    /// machinery roots opened, the structural reductions, the
    /// instantiations built and the property pairs object comparisons
    /// related. Each is counted where the work is done, outside every
    /// charge and its rollback.
    fn member_relation_work(host: &crate::VerterHost) -> [u64; 4] {
        let stats = host.project_type_store().semantic_graph().stats_snapshot();
        [
            stats.relation_roots_opened,
            stats.relation_reductions,
            stats.instantiate_count,
            stats.relation_members_related,
        ]
    }

    fn member_relation_work_since(after: [u64; 4], before: [u64; 4]) -> [u64; 4] {
        std::array::from_fn(|i| after[i] - before[i])
    }

    /// The synchronous premise of the member rows: `Left` is assignable to
    /// `Right` — a complete, priced answer — through one root that relates
    /// both members of the pair and instantiates the later member's
    /// operands.
    fn member_relation_premise(pair: (&str, &str)) -> (ReadSummary, [u64; 4]) {
        let host = member_relation_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (relate, _) = member_relation_keys(&host, &dispatch, pair);
        let before = member_relation_work(&host);
        let read = dispatch.execute_via_cold_build_helper(relate);
        let synchronous = read_summary(&dispatch, &read);
        let work = member_relation_work_since(member_relation_work(&host), before);
        assert!(
            synchronous.0.contains("Assignable") && !synchronous.0.contains("NotAssignable"),
            "premise: the pair is assignable: {synchronous:?}"
        );
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(work[0], 1, "premise: one machinery root: {work:?}");
        assert!(
            work[2] >= 2 && work[3] >= 2,
            "premise: the root relates both members and instantiates the later one's operands: {work:?}"
        );
        (synchronous, work)
    }

    /// What a polled relation root did while its later member's operand
    /// was held.
    struct HeldMemberRelation {
        yielded: bool,
        suspensions: Vec<
            verter_type_engine::project_semantic_dispatch::relation::RelationSuspensionForTests,
        >,
        at_hold: [u64; 4],
        repolled_pending: bool,
        pending_elsewhere: Option<bool>,
        repoll_work: [u64; 4],
        woken: bool,
        work: [u64; 4],
        read: ValueRead,
    }

    /// The relation polled at the production entry on a fresh host while
    /// another task produces its later member's operand instantiation;
    /// released, its task is resumed on another thread and the answer
    /// delivered here.
    fn poll_member_relation_holding(
        pair: (&str, &str),
    ) -> (Arc<crate::VerterHost>, HeldMemberRelation) {
        let host = member_relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, leaf) = member_relation_keys(&host, &dispatch, pair);
        let (other, lease) = hold_producer(&host, graph, leaf);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = member_relation_work(&host);
        let _ = verter_type_engine::project_semantic_dispatch::relation::take_relation_suspensions_for_tests();
        let mut entry = dispatch.poll_query(relate);
        let (first, suspensions, at_hold, repolled_pending, pending_elsewhere, repoll_work) =
            while_held(lease, || {
                let first = entry.poll(&waker);
                let suspensions = verter_type_engine::project_semantic_dispatch::relation::take_relation_suspensions_for_tests();
                let at_hold = member_relation_work_since(member_relation_work(&host), before);
                if first.is_ready() {
                    return (first, suspensions, at_hold, false, None, [0; 4]);
                }
                let repolled_pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(&waker).is_pending()
                };
                let pending_elsewhere = entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                });
                let repoll_work = member_relation_work_since(
                    member_relation_work_since(member_relation_work(&host), before),
                    at_hold,
                );
                (
                    first,
                    suspensions,
                    at_hold,
                    repolled_pending,
                    pending_elsewhere,
                    repoll_work,
                )
            });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => resume_elsewhere(&mut entry),
        };
        drop(entry);
        let run = HeldMemberRelation {
            yielded,
            suspensions,
            at_hold,
            repolled_pending,
            pending_elsewhere,
            repoll_work,
            woken,
            work: member_relation_work_since(member_relation_work(&host), before),
            read,
        };
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(other);
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    /// A relation root whose later member's operand instantiation another
    /// task is producing returns `Pending` at the polled entry from inside
    /// two object comparisons: the root's first member relates the
    /// recursive references `Left` and `Right` name on a frame of its own,
    /// whose comparison's first member re-enters that pair — its back-edge
    /// recorded on that frame — and whose later member's own frame is open
    /// above it, its reduction standing at its structural dispatch. Polled again while held, under
    /// a probe that forbids every wait, and on another thread, it stays
    /// pending and does no work. Released, it resumes on another thread at
    /// that member — relating no member, opening no root and reducing no
    /// pair again — and delivers exactly what the synchronous entry reads,
    /// receipt tree included, with the same absolute work.
    #[test]
    fn a_relation_root_held_at_a_later_member_resumes_after_its_recorded_backedge() {
        let (synchronous, synchronous_work) = member_relation_premise(("Left", "Right"));
        let (host, run) = poll_member_relation_holding(("Left", "Right"));
        assert!(
            run.yielded,
            "the root yields while another task instantiates its later member's operand:              work {:?} (synchronous {synchronous_work:?}), synchronous read {synchronous:?}",
            run.work
        );
        let [suspension] = run.suspensions.as_slice() else {
            panic!("the root's dispatch stops once: {:?}", run.suspensions);
        };
        assert_eq!(
            suspension.member_positions,
            [0, 1],
            "the root's comparison stands at its recursive member, whose own comparison stands at its later member: {suspension:?}"
        );
        let [root, recursive, member] = suspension.frames.as_slice() else {
            panic!("the root, its recursive member and that member's later member are open: {suspension:?}");
        };
        assert!(
            root.assumption_targets.is_empty()
                && recursive.assumption_targets.contains(&1)
                && recursive.min_open_target == Some(1),
            "the recursive pair's back-edge to its own frame is recorded: {suspension:?}"
        );
        assert!(
            member.assumption_targets.is_empty()
                && member.pending_watermark == suspension.pending,
            "the later member's frame is open with nothing deposited above its watermark: {suspension:?}"
        );
        assert_eq!(
            run.at_hold[0], 1,
            "one root is open while the member waits: {:?}",
            run.at_hold
        );
        assert!(
            run.at_hold[3] >= 2,
            "both members began relating before the wait: {:?}",
            run.at_hold
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 4], "a pending repoll does no work");
        assert!(run.woken, "the producer going away wakes the root");
        assert_eq!(
            run.work, synchronous_work,
            "no member relates again, no root opens again and no pair reduces twice"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A relation root whose member's own object comparison reaches the
    /// held read: `OuterLeft` related to `OuterRight` relates `inner`, the
    /// `Left`/`Right` pair, on a frame of its own, whose first member
    /// relates the recursive references on a frame of their own, which
    /// their own first member re-enters and whose later member waits on
    /// `BoxA<"a">`. The root stops three comparisons deep — every frame kept
    /// open, the back-edge recorded on the recursive pair's frame — and
    /// resumes elsewhere to the synchronous read with the same absolute
    /// work.
    #[test]
    fn a_relation_root_held_inside_a_nested_members_comparison_resumes_where_it_stood() {
        let pair = ("OuterLeft", "OuterRight");
        let (synchronous, synchronous_work) = member_relation_premise(pair);
        let (host, run) = poll_member_relation_holding(pair);
        assert!(
            run.yielded,
            "the root yields while the nested member is held"
        );
        let [suspension] = run.suspensions.as_slice() else {
            panic!("the root's dispatch stops once: {:?}", run.suspensions);
        };
        assert_eq!(
            suspension.member_positions,
            [0, 0, 1],
            "the root stands inside the nested comparisons at the later member: {suspension:?}"
        );
        let [root, inner, recursive, leaf] = suspension.frames.as_slice() else {
            panic!("the root, the inner pair, the recursive pair and its later member are open: {suspension:?}");
        };
        assert!(
            root.assumption_targets.is_empty()
                && inner.assumption_targets.is_empty()
                && recursive.assumption_targets.contains(&2)
                && recursive.min_open_target == Some(2)
                && leaf.assumption_targets.is_empty(),
            "the recursive member's back-edge targets the recursive pair's frame: {suspension:?}"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 4], "a pending repoll does no work");
        assert!(run.woken);
        assert_eq!(
            run.work, synchronous_work,
            "no comparison walks again and no frame opens again"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A relation root released while its later member waits on its held
    /// operand closes the member's frame and its own and withdraws its
    /// wait, leaving nothing open; the producer it waited on is left alone,
    /// and the dispatcher reads the relation afterwards as the synchronous
    /// entry does.
    #[test]
    fn a_relation_root_released_while_its_later_member_is_held_leaves_nothing_open() {
        let (synchronous, _) = member_relation_premise(("Left", "Right"));
        let host = member_relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, leaf) = member_relation_keys(&host, &dispatch, ("Left", "Right"));
        let (other, lease) = hold_producer(&host, graph, leaf.clone());
        let _ = verter_type_engine::project_semantic_dispatch::relation::take_relation_suspensions_for_tests();
        let mut entry = dispatch.poll_query(relate.clone());
        let (pending, held_flights) = while_held(lease, || {
            let pending = entry.poll(Waker::noop()).is_pending();
            drop(entry);
            (pending, graph.resident_flight_keys_for_tests())
        });
        assert!(pending, "the root yields while its later member is held");
        assert!(
            held_flights.contains(&leaf),
            "the released root leaves the held producer's flight alone: {held_flights:?}"
        );
        assert_eq!(
            verter_type_engine::project_semantic_dispatch::relation::take_relation_suspensions_for_tests().len(),
            1,
            "the root stopped inside its dispatch"
        );
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "the released root closes its member's frame and its own"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(
            graph.resident_flight_keys_for_tests().is_empty(),
            "the released root's relation flights are withdrawn"
        );
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(relate);
        assert_eq!(read_summary(&dispatch, &read), synchronous);
    }

    /// A relation root whose later member waits on its held operand is
    /// woken by its request's cancellation with the producer still held,
    /// and its next poll delivers the terminal cancelled read, every frame
    /// it opened closed and its wait withdrawn.
    #[test]
    fn a_relation_root_held_at_a_later_member_is_woken_by_its_requests_cancellation() {
        let host = member_relation_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (relate, leaf) = member_relation_keys(&host, &dispatch, ("Left", "Right"));
        let (other, lease) = hold_producer(&host, graph, leaf.clone());
        let request = budgeted_request(1);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        let (first, woken, next, held_flights) = {
            let _request = verter_type_engine::request_context::RequestContextGuard::install(
                Arc::clone(&request),
            );
            let mut entry = dispatch.poll_query(relate);
            let observed = while_held(lease, || {
                let first = entry.poll(&waker).is_pending();
                let before = wakes();
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| request.cancel())
                        .join()
                        .expect("the request is cancelled on another thread");
                });
                let woken = wakes() > before;
                let next = match entry.poll(&waker) {
                    Poll::Ready(read) => Some(read),
                    Poll::Pending => None,
                };
                (first, woken, next, graph.resident_flight_keys_for_tests())
            });
            drop(entry);
            observed
        };
        assert!(first, "the root yields while its later member is held");
        assert!(
            held_flights.contains(&leaf),
            "the cancelled root leaves the held producer's flight alone: {held_flights:?}"
        );
        assert!(
            woken,
            "the request's cancellation wakes the root with the producer still held"
        );
        let read = next.expect("the woken root reads its cancellation without the producer");
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Cancelled)
            ),
            "the cancelled root reads a terminal cancellation: {read:?}"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(
            graph.resident_flight_keys_for_tests().is_empty(),
            "the cancelled root's relation flights are withdrawn"
        );
        drop(other);
    }

    /// The physical work call resolutions on `host` did, wherever they ran:
    /// the candidates whose applicability check began, and the argument
    /// relations their first inference pass ran. Each is counted where the
    /// work is done, outside every charge and its rollback.
    fn call_work(host: &crate::VerterHost) -> [u64; 2] {
        let stats = host.project_type_store().semantic_graph().stats_snapshot();
        [stats.call_candidates_begun, stats.call_arguments_related]
    }

    fn call_work_since(after: [u64; 2], before: [u64; 2]) -> [u64; 2] {
        [after[0] - before[0], after[1] - before[1]]
    }

    /// `pick(1, box)`'s result read through the synchronous entry on a
    /// fresh host, and the call work it did. Its premise: `T` infers from
    /// both arguments, the call's one candidate begun once and each of its
    /// two arguments related once.
    fn synchronous_call() -> (ReadSummary, [u64; 2]) {
        let host = call_host();
        let _request = verter_type_engine::request_context::install_test_request_for(CALL_FILE);
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let before = call_work(&host);
        let read = dispatch.execute_via_cold_build_helper(call_result_key(&dispatch));
        let summary = read_summary(&dispatch, &read);
        let work = call_work_since(call_work(&host), before);
        assert_eq!(
            summary.0, "Union(2 | 1)",
            "premise: what the first argument deposited survives the second: {summary:?}"
        );
        assert_eq!(
            work,
            [1, 2],
            "premise: one candidate begun, both arguments related"
        );
        (summary, work)
    }

    /// What a polled call root did while the reads `held` selects were
    /// produced by another task.
    struct HeldCall {
        /// The first poll returned `Pending` instead of waiting the
        /// producer out.
        yielded: bool,
        /// How many reads another task held for the call's frame.
        held: usize,
        /// The suspensions the call's walk made inside its candidate.
        suspensions: Vec<verter_type_engine::project_semantic_dispatch::CallSuspensionForTests>,
        /// Work done by the first poll.
        at_hold: [u64; 2],
        /// A second poll here while still held — under a probe that
        /// forbids every wait — and then its task polled on another thread
        /// stayed pending.
        repolled_pending: bool,
        pending_elsewhere: Option<bool>,
        /// Work done by those repolls.
        repoll_work: [u64; 2],
        /// The producers going away woke the root.
        woken: bool,
        /// Work done over the whole polled read.
        work: [u64; 2],
        read: ValueRead,
    }

    /// `pick(1, box)`'s result polled at the production entry on a fresh
    /// host while another task produces each read its call frame needs that
    /// `held` selects; released, its task resumes on another thread and the
    /// answer is delivered here.
    fn poll_call_holding(
        held: fn(&SemanticQueryKey) -> bool,
    ) -> (Arc<crate::VerterHost>, HeldCall) {
        let host = call_host();
        // The call is demanded by a request on its own file, as a root a
        // request reads from its file is.
        let _request = verter_type_engine::request_context::install_test_request_for(CALL_FILE);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let key = call_result_key(&dispatch);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let _ = verter_type_engine::project_semantic_dispatch::take_call_suspensions_for_tests();
        let before = call_work(&host);
        let mut entry = dispatch.poll_query(key);
        verter_type_engine::project_semantic_dispatch::hold_call_needs_here_for_tests(Some(held));
        let first = entry.poll(&waker);
        verter_type_engine::project_semantic_dispatch::hold_call_needs_here_for_tests(None);
        let at_hold = call_work_since(call_work(&host), before);
        let suspensions =
            verter_type_engine::project_semantic_dispatch::take_call_suspensions_for_tests();
        let (yielded, held, repolled_pending, pending_elsewhere, repoll_work, read) = match first {
            Poll::Ready(read) => (false, 0, false, None, [0; 2], read),
            Poll::Pending => {
                let repolled_pending = {
                    let _forbid = verter_execution::blocking::WaitProbe::forbid();
                    entry.poll(&waker).is_pending()
                };
                let pending_elsewhere = entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                });
                let repoll_work =
                    call_work_since(call_work_since(call_work(&host), before), at_hold);
                let held = entry
                    .task()
                    .map_or(0, |task| task.release_held_call_needs_for_tests());
                let read = resume_elsewhere(&mut entry);
                (
                    true,
                    held,
                    repolled_pending,
                    pending_elsewhere,
                    repoll_work,
                    read,
                )
            }
        };
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        drop(entry);
        let run = HeldCall {
            yielded,
            held,
            suspensions,
            at_hold,
            repolled_pending,
            pending_elsewhere,
            repoll_work,
            woken,
            work: call_work_since(call_work(&host), before),
            read,
        };
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(scope);
        drop(dispatch);
        (host, run)
    }

    fn instantiation_reads(key: &SemanticQueryKey) -> bool {
        matches!(key, SemanticQueryKey::Instantiate(_))
    }

    fn candidate_list_reads(key: &SemanticQueryKey) -> bool {
        matches!(key, SemanticQueryKey::ResolveOverloadSet { .. })
    }

    /// Sequential reads inside a candidate: a call root whose candidate
    /// already related its first argument — its collector holding what `1`
    /// inferred for `T` — and now waits on an instantiation its second
    /// argument's relation names returns `Pending` with the candidate
    /// retained at that argument. Polled again while held, under a probe
    /// that forbids every wait, and on another thread, it stays pending and
    /// does no work. Released, it resumes on another thread at the second
    /// argument — the candidate is not begun again and the first argument
    /// is not related again — and delivers exactly what the synchronous
    /// entry reads, receipt tree included, with the same absolute work.
    #[test]
    fn a_call_root_waiting_on_its_second_argument_resumes_without_beginning_its_candidate_again() {
        let (synchronous, synchronous_work) = synchronous_call();
        let (host, run) = poll_call_holding(instantiation_reads);
        assert!(
            run.yielded,
            "the call yields while another task instantiates its second argument's operand"
        );
        assert!(run.held > 0);
        let [suspension] = run.suspensions.as_slice() else {
            panic!(
                "the walk stops once inside its candidate: {:?}",
                run.suspensions
            );
        };
        assert!(
            suspension
                .needs
                .iter()
                .all(|need| matches!(need, SemanticQueryKey::Instantiate(_))),
            "it waits on an instantiation `box` against `Box<T>` names: {suspension:?}"
        );
        assert_eq!(
            suspension.collected_candidates, 1,
            "its collector holds what the first argument inferred"
        );
        assert_eq!(
            run.at_hold,
            [1, 1],
            "the candidate began and related its first argument before the wait"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 2], "a pending repoll does no work");
        assert!(run.woken, "the producers going away wake the call");
        assert_eq!(
            run.work, synchronous_work,
            "no candidate begins again and no argument relates twice"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A call root whose candidate list another task is producing returns
    /// `Pending` before any candidate begins, the list its frame's need.
    /// Polled again while held and on another thread, it stays pending and
    /// does no work. Released, it resumes on another thread with the
    /// delivered list — read once — and delivers exactly what the
    /// synchronous entry reads, receipt tree included, with the same
    /// absolute work.
    #[test]
    fn a_call_root_waiting_on_its_candidate_list_resumes_to_the_synchronous_read() {
        let (synchronous, synchronous_work) = synchronous_call();
        let (host, run) = poll_call_holding(candidate_list_reads);
        assert!(
            run.yielded,
            "the call yields while another task produces its candidate list"
        );
        assert!(run.held > 0);
        assert!(
            run.suspensions.is_empty(),
            "no candidate began: {:?}",
            run.suspensions
        );
        assert_eq!(run.at_hold, [0; 2], "no candidate begins before its list");
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll_work, [0; 2], "a pending repoll does no work");
        assert!(run.woken, "the producer going away wakes the call");
        assert_eq!(run.work, synchronous_work);
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A call root waiting inside its candidate on a held instantiation is
    /// woken by its request's cancellation with the producer still held,
    /// and its next poll delivers the terminal cancelled read: its
    /// candidate's collector released, its executor frame closed and its
    /// wait withdrawn, nothing left open.
    #[test]
    fn a_call_root_is_woken_by_its_requests_cancellation_while_its_operand_is_held() {
        let host = call_host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let key = call_result_key(&dispatch);
        let request = budgeted_request(1);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let wakes = || counter.0.load(std::sync::atomic::Ordering::SeqCst);
        let (first, woken, next) = {
            let _request = verter_type_engine::request_context::RequestContextGuard::install(
                Arc::clone(&request),
            );
            let mut entry = dispatch.poll_query(key);
            verter_type_engine::project_semantic_dispatch::hold_call_needs_here_for_tests(Some(
                instantiation_reads,
            ));
            let first = entry.poll(&waker).is_pending();
            verter_type_engine::project_semantic_dispatch::hold_call_needs_here_for_tests(None);
            let before = wakes();
            std::thread::scope(|scope| {
                scope
                    .spawn(|| request.cancel())
                    .join()
                    .expect("the request is cancelled on another thread");
            });
            let woken = wakes() > before;
            let next = match entry.poll(&waker) {
                Poll::Ready(read) => Some(read),
                Poll::Pending => None,
            };
            drop(entry);
            (first, woken, next)
        };
        assert!(first, "the call yields while its operand is held");
        assert!(
            woken,
            "the request's cancellation wakes the call with the producer still held"
        );
        let read = next.expect("the woken call reads its cancellation without the producer");
        assert!(
            matches!(
                read.value(),
                QueryResult::Error(verter_type_engine::semantic_query::QueryError::Cancelled)
            ),
            "the cancelled call reads a terminal cancellation: {read:?}"
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    }

    const PRIMITIVE_CALL_FILE: &str = "/semantic-task/primitive-call.ts";

    /// A call whose callee is a primitive: `string`'s call signatures are
    /// those of the `String` interface the call site's project declares.
    const PRIMITIVE_CALL: &str =
        "declare global { interface String { (radix: number): boolean } }\n\
                                  declare const s: string;\n\
                                  const r = s(1);\n\
                                  export {};\n";

    /// `s(1)`'s result read on a fresh host with no request to scope it:
    /// through the synchronous entry, or polled at the production entry.
    fn primitive_call_read(polled: bool) -> ReadSummary {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            PRIMITIVE_CALL_FILE,
            PRIMITIVE_CALL,
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(PRIMITIVE_CALL_FILE)
            .expect("the call has indexed input");
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let key = dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    PRIMITIVE_CALL_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: verter_type_expr::OwnedStr::from("r"),
            },
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        );
        let read = if polled {
            let mut entry = dispatch.poll_query(key);
            let read = poll_until_ready(|waker| entry.poll(waker));
            drop(entry);
            read
        } else {
            dispatch.execute_via_cold_build_helper(key)
        };
        read_summary(&dispatch, &read)
    }

    /// A primitive callee's candidate list is read in the call site's
    /// scope wherever the call's walk runs: on a frame of the continuation
    /// runtime with no request to scope what the runtime reads, the walk
    /// reads the list where it stands, under its call site, and `s(1)`
    /// selects `(radix: number): boolean` exactly as the synchronous entry
    /// does.
    #[test]
    fn a_primitive_callees_candidate_list_is_read_in_its_call_sites_scope() {
        let synchronous = primitive_call_read(false);
        assert!(
            matches!(
                synchronous.0.as_str(),
                "boolean" | "Union(false | true)" | "Union(true | false)"
            ),
            "premise: the call selects the project's `String` call signature: {synchronous:?}"
        );
        assert_eq!(primitive_call_read(true), synchronous);
    }

    /// `alias<args>` over the aliases' file.
    fn projection_application(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        alias: &str,
        arg: verter_type_engine::semantic_query::SemanticNodeId,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        use verter_type_engine::semantic_query::{DeclIdentity, PayloadArray};
        dispatch
            .graph_arc()
            .intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: PROJECTION_FILE.into(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: host
                        .shallow_file_state(PROJECTION_FILE)
                        .expect("the aliases are indexed")
                        .whole_hash,
                    decl_name: alias.into(),
                },
                args: PayloadArray::from(vec![arg]),
            })
    }

    /// The path `["value"]` over the deferred access
    /// `Pair<Pair<"deep">>["inner"]["value"]["inner"]`, read in `Navigate`,
    /// and the instantiations its nested operand's evaluation reads, in the
    /// order it reads them. The walk stands at the outer access, whose
    /// object `Pair<Pair<"deep">>["inner"]["value"]` is itself a deferred
    /// access: the shared deferred evaluator reduces it, reading
    /// `Pair<Pair<"deep">>["inner"]` (which instantiates
    /// `Pair<Pair<"deep">>`) and then `Box<Pair<"deep">>["value"]` (which
    /// instantiates `Box<Pair<"deep">>`), before the hop reads the outer
    /// access over its answer `Pair<"deep">`.
    fn deferred_operand_path(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, [SemanticQueryKey; 2]) {
        use verter_type_engine::semantic_query::{
            IndexKey, InstantiateKey, LiteralValue, PathSegment, PropertyKey,
        };
        let graph = dispatch.graph_arc();
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let deep = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "deep".into(),
        )));
        let pair = projection_application(host, dispatch, "Pair", deep);
        let pair_of_pair = projection_application(host, dispatch, "Pair", pair);
        let access = |object, index: &str| {
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object,
                index: IndexKey::from(index),
                origin: verter_type_engine::semantic_query::IndexedAccessOrigin::Composed,
            })
        };
        let inner = access(pair_of_pair, "inner");
        let value = access(inner, "value");
        let base = access(value, "inner");
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let path = SemanticQueryKey::ProjectPath {
            base,
            path: verter_type_expr::OwnedSlice::from(
                vec![PathSegment::Member(PropertyKey::identifier("value"))].into_boxed_slice(),
            ),
            context,
        };
        let hop = |alias: &str| {
            SemanticQueryKey::Instantiate(InstantiateKey::new(
                dispatch.type_slot_for(PROJECTION_FILE, owner, alias),
                Arc::from([pair]),
                dispatch.instantiate_context_for(
                    PROJECTION_FILE,
                    context.into_structural_transit_with_mode(ProjectionMode::Navigate),
                ),
            ))
        };
        (path, [hop("Pair"), hop("Box")])
    }

    /// The steps the nested operand's evaluation computes: the outer
    /// access pushing its object (1); the object access pushing
    /// `Pair<Pair<"deep">>`, which evaluates to itself in place (2); the
    /// object access reading `Pair<Pair<"deep">>["inner"]` (3) and finishing
    /// on its answer `Box<Pair<"deep">>` (4); the outer access reading
    /// `Box<Pair<"deep">>["value"]` (5) and finishing on its answer
    /// `Pair<"deep">` (6).
    const DEFERRED_OPERAND_STEPS: u64 = 6;

    /// The step at which the evaluation names its second read.
    const DEFERRED_OPERAND_SECOND_READ_STEP: u64 = 5;

    /// A path whose hop stands at a deferred access whose object is itself
    /// deferred evaluates that object on the shared deferred evaluator, as
    /// an owned sub-continuation of the hop. Polled while another task
    /// instantiates `Pair<Pair<"deep">>` — the evaluation's first read,
    /// which the evaluator reads through the `IndexedAccess` sugar — the
    /// path returns `Pending` with the evaluation retained, instead of
    /// waiting the producer out where the evaluator stands. Repolled while
    /// held, under a probe that forbids every wait, and on another thread,
    /// it computes nothing; released, it resumes the evaluation at its held
    /// read and delivers exactly what the synchronous entry reads, receipt
    /// tree included, with each step and hop computed once (absolute).
    #[test]
    fn a_hop_whose_deferred_operand_reads_a_held_instantiation_yields_and_resumes_to_the_synchronous_read(
    ) {
        let (synchronous, synchronous_work) =
            synchronous_root(|host, dispatch| deferred_operand_path(host, dispatch).0);
        assert_eq!(synchronous.0, "\"deep\"", "{synchronous:?}");
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(synchronous_work.deferred, DEFERRED_OPERAND_STEPS);

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (path, hops) = deferred_operand_path(host, dispatch);
            (path, hops[0].clone())
        });
        assert!(
            run.yielded,
            "the path yields while its operand's first read is held"
        );
        assert_eq!(
            run.at_hold.deferred, 3,
            "the evaluation stands at its first read: both accesses pushed, the inner one read"
        );
        assert!(run.repolled_pending, "a repoll while held enters no wait");
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken, "the producer going away wakes the path");
        assert_eq!(
            run.total, synchronous_work,
            "no step or hop is computed again: the evaluation resumes at its held read"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// Sequential reads inside one helper: the nested operand's evaluation
    /// already read `Pair<Pair<"deep">>["inner"]` and finished its inner
    /// frame, and now reads `Box<Pair<"deep">>["value"]` while another task
    /// instantiates `Box<Pair<"deep">>`. The path returns `Pending` holding
    /// the completed frame's answer; resumed elsewhere, the evaluation
    /// continues at the second read — its completed steps and first read
    /// are not computed again, counted as physical steps where they run,
    /// outside every charge and its rollback — and the path delivers
    /// exactly what the synchronous entry reads.
    #[test]
    fn a_deferred_operand_waiting_on_its_second_read_resumes_without_evaluating_its_completed_steps_again(
    ) {
        let (synchronous, synchronous_work) =
            synchronous_root(|host, dispatch| deferred_operand_path(host, dispatch).0);
        assert_eq!(synchronous_work.deferred, DEFERRED_OPERAND_STEPS);

        let (host, run) = poll_root_holding(|host, dispatch| {
            let (path, hops) = deferred_operand_path(host, dispatch);
            (path, hops[1].clone())
        });
        assert!(
            run.yielded,
            "the path yields while its operand's second read is held"
        );
        assert_eq!(
            run.at_hold.deferred, DEFERRED_OPERAND_SECOND_READ_STEP,
            "the inner frame completed before the wait: the outer access stands at its read"
        );
        assert!(run.repolled_pending);
        assert_eq!(run.pending_elsewhere, Some(true));
        assert_eq!(run.repoll, NO_WORK, "a pending repoll computes nothing");
        assert!(run.woken);
        assert_eq!(
            run.total, synchronous_work,
            "the completed steps are not evaluated again"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous);
    }

    /// A path released while its nested operand's evaluation waits on a
    /// held read withdraws its wait and leaves nothing open — no cost
    /// recording of the evaluation's frames, no wait edge — and the
    /// dispatcher reads the path afterwards as the synchronous entry does.
    #[test]
    fn a_path_released_while_its_deferred_operand_is_held_leaves_nothing_open() {
        let (synchronous, _) =
            synchronous_root(|host, dispatch| deferred_operand_path(host, dispatch).0);
        for held in 0..2 {
            let host = projection_host();
            let mut dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let graph = host.project_type_store().semantic_graph();
            let (path, hops) = deferred_operand_path(&host, &dispatch);
            let (other, lease) = hold_producer(&host, graph, hops[held].clone());
            let mut entry = dispatch.poll_query(path.clone());
            let pending = while_held(lease, || {
                let pending = entry.poll(Waker::noop()).is_pending();
                drop(entry);
                pending
            });
            assert!(pending, "the path yields while read {held} is held");
            assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
            assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
            drop(other);
            let read = dispatch.execute_via_cold_build_helper(path);
            assert_eq!(read_summary(&dispatch, &read).0, synchronous.0);
        }
    }

    /// The file whose declarations the walker's hop sites are exercised
    /// over: aliases a path walks through, a homomorphic mapping, an
    /// intersection and a union over arrays, and the global interfaces a
    /// member read off an array or an object widens to.
    const SITES_FILE: &str = "/semantic-task/projection-sites.ts";

    const SITES: &str = "declare global {\n\
                           interface Array<T> { readonly marker: \"arr\"; }\n\
                           interface Object { readonly objectMarker: \"obj\"; }\n\
                         }\n\
                         type Box<T> = { value: T };\n\
                         type Pair<T> = { inner: Box<T> };\n\
                         type List<T> = { items: T[] };\n\
                         type Mapped<T> = { [K in keyof T]: T[K] };\n\
                         type Tagged<T> = { [K in keyof T]: Box<K> };\n\
                         type Both<T> = { a: T } & T[];\n\
                         type Either<T> = T[] | [T];\n\
                         type Plain = { plain: \"p\" };\n\
                         type Other = { other: \"o\" };\n";

    fn sites_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            SITES_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(SITES),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(SITES_FILE)
            .expect("the declarations have indexed input before a hop is held");
        host
    }

    fn sites_scope(host: &crate::VerterHost) -> verter_type_engine::semantic_query::NodeScopeId {
        verter_type_engine::semantic_query::NodeScopeId::File {
            canonical_id: SITES_FILE.into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: host
                .shallow_file_state(SITES_FILE)
                .expect("the declarations are indexed")
                .whole_hash,
            local_scope: None,
        }
    }

    fn sites_literal(
        dispatch: &HostDispatch<'_>,
        text: &str,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        dispatch.graph_arc().intern_node(SemanticNodeData::Literal(
            verter_type_engine::semantic_query::LiteralValue::String(text.into()),
        ))
    }

    /// `alias<arg>` written in the sites file: the walk's subject, scoped
    /// to the file it is read in.
    fn sites_application(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        alias: &str,
        arg: verter_type_engine::semantic_query::SemanticNodeId,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        use verter_type_engine::semantic_query::{DeclIdentity, PayloadArray};
        dispatch.graph_arc().intern_node_with_scope(
            SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: SITES_FILE.into(),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: host
                        .shallow_file_state(SITES_FILE)
                        .expect("the declarations are indexed")
                        .whole_hash,
                    decl_name: alias.into(),
                },
                args: PayloadArray::from(vec![arg]),
            },
            sites_scope(host),
        )
    }

    /// `alias<args>` instantiated in the sites file under `context`.
    fn sites_instantiation(
        dispatch: &HostDispatch<'_>,
        alias: &str,
        args: &[verter_type_engine::semantic_query::SemanticNodeId],
        context: ProjectionReductionContext,
    ) -> SemanticQueryKey {
        SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
            dispatch.type_slot_for(
                SITES_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                alias,
            ),
            Arc::from(args.to_vec().into_boxed_slice()),
            dispatch.instantiate_context_for(SITES_FILE, context),
        ))
    }

    fn member_path(
        names: &[&str],
    ) -> verter_type_expr::OwnedSlice<verter_type_engine::semantic_query::PathSegment> {
        use verter_type_engine::semantic_query::{PathSegment, PropertyKey};
        verter_type_expr::OwnedSlice::from(
            names
                .iter()
                .map(|name| PathSegment::Member(PropertyKey::identifier(*name)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    }

    /// A path whose walk reaches a hop site, and the reads its walk names
    /// up to and including that site's, in order.
    type SiteKeys<'k> = &'k dyn Fn(&crate::VerterHost, &HostDispatch<'_>) -> SiteRead;

    /// A path and the reads its walk names, in order.
    type SiteRead = (SemanticQueryKey, Vec<SemanticQueryKey>);

    /// `Alias(Pair<"deep">)["inner"]["value"]` in `Expanded`, its `Pair` an
    /// unresolved reference the walk reaches behind an alias, not as its
    /// subject: the walk resolves the carrier mid-walk, reading its anchor
    /// declaration and then the instantiation it routes through.
    fn mid_walk_carrier(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let graph = dispatch.graph_arc();
        let deep = sites_literal(dispatch, "deep");
        let scope = sites_scope(host);
        let carrier = graph.intern_node_with_scope(
            SemanticNodeData::new_bare_ref(Arc::from("Pair"), scope.clone(), Arc::from([deep])),
            scope,
        );
        let base = graph.intern_node(SemanticNodeData::Alias(carrier));
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let root = SemanticQueryKey::ProjectPath {
            base,
            path: member_path(&["inner", "value"]),
            context,
        };
        let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
        let anchor =
            SemanticQueryKey::ResolveDecl(verter_type_engine::semantic_query::ResolveDeclKey {
                scope: ScopeId::file(SITES_FILE, owner),
                name: "Pair".into(),
            });
        let instantiation = sites_instantiation(dispatch, "Pair", &[deep], context);
        (root, vec![anchor, instantiation])
    }

    /// `Pair<"deep">["inner"]` in `Expanded`: the walk instantiates its
    /// base, reads `inner` off it, and resolves the terminal `Box<"deep">`
    /// one level under the caller's mode before expanding it.
    fn expanded_terminal(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let deep = sites_literal(dispatch, "deep");
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "Pair", deep),
            path: member_path(&["inner"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "Pair",
            &[deep],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let terminal = sites_instantiation(dispatch, "Box", &[deep], context);
        (root, vec![base, terminal])
    }

    /// `List<"deep">["items"]["marker"]` in `Navigate`: the walk
    /// instantiates its base, reads the array `items`, and reads `marker`
    /// off the array's apparent wrapper, the global `Array` over its
    /// element, in the file the subject was written in.
    fn array_wrapper(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let deep = sites_literal(dispatch, "deep");
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "List", deep),
            path: member_path(&["items", "marker"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "List",
            &[deep],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let wrapper = dispatch
            .global_wrapper_read_for_tests("Array", &[deep], SITES_FILE)
            .expect("the sites file declares the global `Array`");
        (root, vec![base, wrapper])
    }

    /// `alias<Box<"deep">>["value"]` in `Navigate`, `alias` a mapping
    /// homomorphic over its argument, and the reads its walk names before
    /// the selected key's value: the mapping's instantiation, its source's
    /// carrier chain — whether the source is an array or tuple the mapping
    /// maps whole —, the evaluation of its key space (`keyof Box<"deep">`)
    /// admitting the selected key, then the source's carrier chain again to
    /// admit the key against the source's shallow surface.
    fn mapped_selection(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        alias: &str,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let deep = sites_literal(dispatch, "deep");
        let boxed = sites_application(host, dispatch, "Box", deep);
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, alias, boxed),
            path: member_path(&["value"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            alias,
            &[boxed],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let source = |mode| {
            sites_instantiation(
                dispatch,
                "Box",
                &[deep],
                context.into_structural_transit_with_mode(mode),
            )
        };
        (
            root,
            vec![
                base,
                source(ProjectionMode::Navigate),
                SemanticQueryKey::KeyOf {
                    base: boxed,
                    context: ProjectionReductionContext::published(ProjectionMode::Expanded),
                },
                source(ProjectionMode::Shallow),
            ],
        )
    }

    /// `Mapped<Box<"deep">>["value"]` in `Navigate`: the walk instantiates
    /// the mapping, resolves its source's carrier chain, and narrows it to
    /// the selected key, whose value an identity mapper reads off its
    /// source: `Box<"deep">["value"]`.
    fn mapped_key(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let (root, mut reads) = mapped_selection(host, dispatch, "Mapped");
        let deep = sites_literal(dispatch, "deep");
        let value = sites_literal(dispatch, "value");
        reads.push(SemanticQueryKey::IndexedAccess {
            base: sites_application(host, dispatch, "Box", deep),
            index: verter_type_engine::semantic_query::IndexKey::Computed(value),
            mode: ProjectionMode::Navigate,
        });
        (root, reads)
    }

    /// `Tagged<Box<"deep">>["value"]` in `Navigate`: as [`mapped_key`], but
    /// a computed mapper's selected value is `Box<"value">`, evaluated as a
    /// mapped member's — reading its instantiation.
    fn tagged_key(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let (root, mut reads) = mapped_selection(host, dispatch, "Tagged");
        let value = sites_literal(dispatch, "value");
        reads.push(sites_instantiation(
            dispatch,
            "Box",
            &[value],
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        ));
        (root, reads)
    }

    /// The reads `keys`'s path names, its build driven in place on a
    /// scratch host: the case's reads must be exactly its first ones.
    fn assert_site_reads(keys: SiteKeys<'_>, label: &str) -> Vec<SemanticQueryKey> {
        assert_site_reads_on(sites_host, keys, label)
    }

    /// The same, on a fresh host `host` makes.
    fn assert_site_reads_on(
        host: fn() -> Arc<crate::VerterHost>,
        keys: SiteKeys<'_>,
        label: &str,
    ) -> Vec<SemanticQueryKey> {
        let host = host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (root, reads) = keys(&host, &dispatch);
        let needs = dispatch.family_needs_for_tests(&root);
        assert!(
            needs.len() >= reads.len() && needs[..reads.len()] == reads[..],
            "{label}: the walk names the case's reads first\nnamed: {needs:#?}\nexpected: {reads:#?}"
        );
        needs
    }

    /// The case's path polled while another task produces its read
    /// `held`: it yields there instead of waiting the producer out, a
    /// repoll and a poll elsewhere compute nothing, and released, it
    /// resumes on another thread to exactly what the synchronous entry
    /// reads — answer, rails, diagnostics and receipt tree — computing each
    /// hop, admission stage and evaluator step once (absolute counts). The
    /// work the first poll computed is returned.
    fn assert_site_suspends(keys: SiteKeys<'_>, held: usize, label: &str) -> Work {
        assert_site_suspends_on(sites_host, keys, held, false, label).0
    }

    /// The same, on a fresh host `host` makes, the held read also held at
    /// the site an expression claims it when `expression_site`: the work
    /// the first poll computed, and the work the whole read computed.
    fn assert_site_suspends_on(
        host: fn() -> Arc<crate::VerterHost>,
        keys: SiteKeys<'_>,
        held: usize,
        expression_site: bool,
        label: &str,
    ) -> (Work, Work) {
        let (synchronous, synchronous_work) =
            synchronous_root_on(host, |host, dispatch| keys(host, dispatch).0);
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "{label}: premise: a complete, priced answer: {synchronous:?}"
        );
        let (host, run) = poll_root_holding_at(
            host,
            |host, dispatch| {
                let (root, reads) = keys(host, dispatch);
                (root, reads[held].clone())
            },
            expression_site,
        );
        // No dependency of the walk is joined or driven in place. Only a
        // `DeclLowering` wait is allowed: an instantiation frame's begin
        // lowers its declaration and augmentations in place (see the
        // architecture notes).
        assert!(
            run.first_waits
                .iter()
                .all(|family| *family == verter_execution::blocking::WaitFamily::DeclLowering),
            "{label}: the first poll joins no dependency in place: {:?}",
            run.first_waits
        );
        assert!(
            run.yielded,
            "{label}: the path yields at its held read {held}"
        );
        assert!(
            run.repolled_pending,
            "{label}: a repoll while held enters no wait"
        );
        assert_eq!(run.pending_elsewhere, Some(true), "{label}");
        assert_eq!(
            run.repoll, NO_WORK,
            "{label}: a pending repoll computes nothing"
        );
        assert!(run.woken, "{label}: the producer going away wakes the path");
        assert_eq!(
            run.total, synchronous_work,
            "{label}: nothing completed before the wait is computed again"
        );
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &run.read), synchronous, "{label}");
        (run.at_hold, run.total)
    }

    /// The case's path released while it waits on its held read `held`
    /// withdraws its wait and leaves nothing open — no wait edge, no open
    /// cost recording, no carrier left on the normalization record — and
    /// the dispatcher reads it afterwards as the synchronous entry does.
    fn assert_site_releases(keys: SiteKeys<'_>, held: usize, label: &str) {
        assert_site_releases_on(sites_host, keys, held, false, label);
    }

    /// The same, on a fresh host `host` makes, the held read also held at
    /// the site an expression claims it when `expression_site`.
    fn assert_site_releases_on(
        host: fn() -> Arc<crate::VerterHost>,
        keys: SiteKeys<'_>,
        held: usize,
        expression_site: bool,
        label: &str,
    ) {
        let (synchronous, _) = synchronous_root_on(host, |host, dispatch| keys(host, dispatch).0);
        let host = host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let (root, reads) = keys(&host, &dispatch);
        let (other, lease) = hold_seam(&host, graph, reads[held].clone(), expression_site);
        let mut entry = dispatch.poll_query(root.clone());
        let pending = while_held(lease, || {
            let pending = entry.poll(Waker::noop()).is_pending();
            drop(entry);
            pending
        });
        assert!(
            pending,
            "{label}: the path yields while read {held} is held"
        );
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "{label}"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "{label}");
        drop(other);
        let read = dispatch.execute_via_cold_build_helper(root);
        assert_eq!(read_summary(&dispatch, &read).0, synchronous.0, "{label}");
    }

    /// A walk that reaches an unresolved-reference carrier behind an alias
    /// resolves it as a hop of its own: each read the carrier's resolution
    /// names — its anchor declaration, then the instantiation it routes
    /// through — is the path's dependency, with the resolution and what it
    /// observed retained. Held at either read, the path yields having
    /// walked only the alias and the carrier's hop; released, it resumes
    /// the resolution at the held read — the anchor is not read again —
    /// and delivers exactly what the synchronous entry reads.
    #[test]
    fn a_carrier_reached_mid_walk_yields_at_each_read_of_its_resolution_and_resumes_there() {
        let label = "mid-walk carrier";
        assert_site_reads(&mid_walk_carrier, label);
        for held in 0..2 {
            let at_hold = assert_site_suspends(&mid_walk_carrier, held, label);
            assert_eq!(
                at_hold.hops, 2,
                "{label}: the alias and the carrier's hop were walked before read {held}"
            );
            assert_eq!(at_hold.admission, 0, "{label}: the subject is no carrier");
        }
        for held in 0..2 {
            assert_site_releases(&mid_walk_carrier, held, label);
        }
    }

    /// A non-empty `Expanded` path resolves its terminal carrier one level
    /// as a read of its own. Held there — after the base's instantiation
    /// and its member hop completed — the path yields holding them;
    /// released, it resolves and expands the terminal without walking a
    /// hop again, delivering exactly what the synchronous entry reads.
    #[test]
    fn an_expanded_terminal_waiting_on_its_carriers_instantiation_resumes_without_walking_again() {
        let label = "expanded terminal";
        assert_site_reads(&expanded_terminal, label);
        let at_hold = assert_site_suspends(&expanded_terminal, 1, label);
        assert_eq!(
            at_hold.hops, 2,
            "{label}: the base and its member were walked before the terminal's read"
        );
        assert_site_releases(&expanded_terminal, 1, label);
    }

    /// A member read off an array reads the global `Array` wrapper's
    /// instantiation as the hop's own read, the global lookup answered as
    /// a port. Held there, after the base's instantiation and the array
    /// member, the path yields; released, it reads `marker` off the wrapper
    /// without walking a hop again.
    #[test]
    fn an_array_member_waiting_on_its_wrapper_resumes_without_walking_again() {
        let label = "array wrapper";
        assert_site_reads(&array_wrapper, label);
        let at_hold = assert_site_suspends(&array_wrapper, 1, label);
        assert_eq!(
            at_hold.hops, 3,
            "{label}: the base, its array member and the wrapper's hop were walked"
        );
        assert_site_releases(&array_wrapper, 1, label);
    }

    /// A mapped type narrowed to its selected key reads, for an identity
    /// mapper, the key's value off its source as the hop's own read. Held
    /// there (as the canonical path its entry admits), after the mapping's
    /// instantiation, the path yields; released,
    /// it finishes the key without walking a hop again, to exactly what the
    /// synchronous entry reads.
    #[test]
    fn a_mapped_key_waiting_on_its_selected_value_resumes_without_walking_again() {
        let label = "mapped key";
        assert_site_reads(&mapped_key, label);
        // The selected value is an `IndexedAccess` read, which a frame
        // admits as its canonical path and then needs: that path is held.
        let admitted = |host: &crate::VerterHost, dispatch: &HostDispatch<'_>| {
            let (root, mut reads) = mapped_key(host, dispatch);
            let SemanticQueryKey::IndexedAccess { base, index, mode } = reads[4].clone() else {
                unreachable!("the selected value is an indexed access")
            };
            reads[4] = SemanticQueryKey::ProjectPath {
                base,
                path: verter_type_expr::OwnedSlice::from(
                    vec![verter_type_engine::semantic_query::PathSegment::Index(
                        index,
                    )]
                    .into_boxed_slice(),
                ),
                context: ProjectionReductionContext::published(mode),
            };
            (root, reads)
        };
        assert_site_suspends(&admitted, 4, label);
        assert_site_releases(&admitted, 4, label);
    }

    /// A mapped hop resolves its source's carrier chain as a
    /// sub-continuation of its own: the source's instantiation is the
    /// path's dependency, read to decide whether the mapping maps an array
    /// or tuple whole. Held there, after the mapping's instantiation, the
    /// path yields holding the chain — joining nothing in place; released,
    /// it resumes the chain at the held read, nothing before it computed
    /// again, to exactly what the synchronous entry reads.
    ///
    /// The chain's second read, admitting the key against the source's
    /// shallow surface, is not held here: the key space's evaluation, which
    /// comes first, settles its `keyof` carrier's keys in place, reading that
    /// instantiation.
    #[test]
    fn a_mapped_sources_carrier_chain_waiting_on_its_instantiation_resumes_there() {
        let label = "mapped source";
        assert_site_suspends(&mapped_key, 1, label);
        assert_site_releases(&mapped_key, 1, label);
    }

    /// A mapped hop admits a selected key against a primitive key space as
    /// a sub-continuation of its own: the key space's evaluation is the
    /// path's dependency, the admission retained. Held there, after the
    /// mapping's instantiation and its source's carrier chain, the path
    /// yields — evaluating nothing in place; released, it resumes the
    /// admission at the held read, nothing before it computed again, to
    /// exactly what the synchronous entry reads.
    #[test]
    fn a_mapped_key_space_waiting_on_its_evaluation_resumes_there() {
        let label = "mapped key space";
        assert_site_suspends(&mapped_key, 2, label);
        assert_site_releases(&mapped_key, 2, label);
    }

    /// A computed mapper's selected value is evaluated as a sub-continuation
    /// of the hop: the value's instantiation is the path's dependency, the
    /// evaluation retained. Held there, after the mapping and its source
    /// resolved, the path yields; released, it resumes the evaluation at
    /// the held read and finishes the key without walking a hop again, to
    /// exactly what the synchronous entry reads.
    #[test]
    fn a_computed_mapped_value_waiting_on_its_evaluation_resumes_there() {
        let label = "computed mapped value";
        assert_site_reads(&tagged_key, label);
        assert_site_suspends(&tagged_key, 4, label);
        assert_site_releases(&tagged_key, 4, label);
    }

    /// Every read the case names is among the reads its path's build,
    /// driven in place on a scratch host, names — in whatever order its
    /// arms read them.
    fn assert_site_names(keys: SiteKeys<'_>, label: &str) {
        let host = sites_host();
        let dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let (root, reads) = keys(&host, &dispatch);
        let needs = dispatch.family_needs_for_tests(&root);
        for read in &reads {
            assert!(
                needs.contains(read),
                "{label}: the walk names the read\nread: {read:#?}\nnamed: {needs:#?}"
            );
        }
    }

    /// The global `Object` a member read in the sites file widens to.
    fn sites_object(dispatch: &HostDispatch<'_>) -> SemanticQueryKey {
        dispatch
            .global_wrapper_read_for_tests("Object", &[], SITES_FILE)
            .expect("the sites file declares the global `Object`")
    }

    /// `Box<"deep">["objectMarker"]` in `Navigate`: the walk instantiates
    /// its base and, the object declaring no such member, reads it off the
    /// global `Object` in the file the subject was written in.
    fn object_member(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let deep = sites_literal(dispatch, "deep");
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "Box", deep),
            path: member_path(&["objectMarker"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "Box",
            &[deep],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        (root, vec![base, sites_object(dispatch)])
    }

    /// `Both<"deep">["objectMarker"]` in `Navigate`: the walk instantiates
    /// the intersection `{ a: "deep" } & "deep"[]`, whose arms declare no
    /// such member: the array arm reads it past the global `Array` over
    /// its element, and the join decides it from the members the arms'
    /// global `Object` declares.
    fn intersection_object_member(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let deep = sites_literal(dispatch, "deep");
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "Both", deep),
            path: member_path(&["objectMarker"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "Both",
            &[deep],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let array = dispatch
            .global_wrapper_read_for_tests("Array", &[deep], SITES_FILE)
            .expect("the sites file declares the global `Array`");
        (root, vec![base, array, sites_object(dispatch)])
    }

    /// `Either<"deep">[0]` in `Navigate`: the walk instantiates the union
    /// `"deep"[] | ["deep"]`, whose array arm reports its view of the key
    /// through the global `Array` over its element.
    fn union_arm_view(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        use verter_type_engine::semantic_query::{CanonicalIndexInt, IndexKey, PathSegment};
        let deep = sites_literal(dispatch, "deep");
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "Either", deep),
            path: verter_type_expr::OwnedSlice::from(
                vec![PathSegment::Index(IndexKey::Number(
                    CanonicalIndexInt::from_canonical_i64(0).expect("canonical"),
                ))]
                .into_boxed_slice(),
            ),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "Either",
            &[deep],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let array = dispatch
            .global_wrapper_read_for_tests("Array", &[deep], SITES_FILE)
            .expect("the sites file declares the global `Array`");
        (root, vec![base, array])
    }

    /// The object literal `{ ...Box<"deep"> }` written in the sites file:
    /// a spread program the walk reads a member of through its projection.
    fn sites_spread_program(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        use verter_type_engine::semantic_query::{
            ObjectConstructionEffect, ObjectSpreadProgram, PayloadArray,
        };
        let deep = sites_literal(dispatch, "deep");
        let boxed = sites_application(host, dispatch, "Box", deep);
        dispatch.graph_arc().intern_node_with_scope(
            SemanticNodeData::ObjectSpreadProgram(ObjectSpreadProgram {
                is_literal: true,
                effects: PayloadArray::from(vec![ObjectConstructionEffect::Spread(boxed)]),
            }),
            sites_scope(host),
        )
    }

    /// `{ ...Box<"deep"> }["value"]` in `Navigate`: the walk's first hop
    /// reads the spread program's projection for `value`.
    fn spread_member(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let program = sites_spread_program(host, dispatch);
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: program,
            path: member_path(&["value"]),
            context,
        };
        let projection = dispatch.object_spread_member_key_for_tests(
            program,
            verter_type_engine::semantic_query::PropertyKey::identifier("value"),
            context,
        );
        (root, vec![projection])
    }

    /// `Box<{ ...Box<"deep"> }>["value"]["value"]` in `Navigate`: the walk
    /// instantiates its base, reads the spread program `value`, and reads
    /// the program's projection for `value`.
    fn spread_member_behind_box(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let program = sites_spread_program(host, dispatch);
        let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
        let root = SemanticQueryKey::ProjectPath {
            base: sites_application(host, dispatch, "Box", program),
            path: member_path(&["value", "value"]),
            context,
        };
        let base = sites_instantiation(
            dispatch,
            "Box",
            &[program],
            context.into_structural_transit_with_mode(ProjectionMode::Navigate),
        );
        let projection = dispatch.object_spread_member_key_for_tests(
            program,
            verter_type_engine::semantic_query::PropertyKey::identifier("value"),
            context,
        );
        (root, vec![base, projection])
    }

    /// A member an object does not declare reads the global `Object`'s
    /// instantiation as the hop's own read. Held there, after the base's
    /// instantiation, the path yields having walked the base and the
    /// member's hop; released, it reads the member off `Object` without
    /// walking a hop again, to exactly what the synchronous entry reads.
    #[test]
    fn an_object_member_waiting_on_the_global_object_resumes_without_walking_again() {
        let label = "object member";
        assert_site_reads(&object_member, label);
        let at_hold = assert_site_suspends(&object_member, 1, label);
        assert_eq!(
            at_hold.hops, 2,
            "{label}: the base and the member's hop were walked"
        );
        assert_site_releases(&object_member, 1, label);
    }

    /// An intersection's member its arms do not declare reads each arm's
    /// global wrapper as a dependency of the walk — the array arm's
    /// `Array`, then the global `Object` its join decides the member from,
    /// the join's decision retained across the read. Held at either, the
    /// path yields; released, it resumes at the held read, computing
    /// nothing twice, to exactly what the synchronous entry reads.
    #[test]
    fn an_intersection_join_waiting_on_its_arms_wrappers_resumes_at_the_held_read() {
        let label = "intersection join";
        assert_site_names(&intersection_object_member, label);
        for held in 1..3 {
            assert_site_suspends(&intersection_object_member, held, label);
            assert_site_releases(&intersection_object_member, held, label);
        }
    }

    /// A union's array arm reports its view of a numeric key through the
    /// global `Array`, read as the walk's dependency. Held there, the path
    /// yields; released, it finishes the arm and the join without walking
    /// a hop again.
    #[test]
    fn a_union_arm_view_waiting_on_its_array_wrapper_resumes_without_walking_again() {
        let label = "union arm view";
        assert_site_names(&union_arm_view, label);
        assert_site_suspends(&union_arm_view, 1, label);
        assert_site_releases(&union_arm_view, 1, label);
    }

    /// A member read off a spread program reads the program's projection
    /// for the key as the hop's own read — first (the program the path's
    /// subject) and later (the program reached behind an instantiation).
    /// Held there, the path yields having walked only the hops before it;
    /// released, it resumes at the projection, to exactly what the
    /// synchronous entry reads.
    #[test]
    fn a_spread_member_waiting_on_its_projection_resumes_without_walking_again() {
        let label = "spread member";
        assert_site_reads(&spread_member, label);
        let at_hold = assert_site_suspends(&spread_member, 0, label);
        assert_eq!(
            at_hold.hops, 1,
            "{label}: only the program's hop was walked"
        );
        assert_site_releases(&spread_member, 0, label);

        let label = "spread member behind an instantiation";
        assert_site_reads(&spread_member_behind_box, label);
        let at_hold = assert_site_suspends(&spread_member_behind_box, 1, label);
        assert_eq!(
            at_hold.hops, 3,
            "{label}: the base, its member and the program's hop were walked"
        );
        assert_site_releases(&spread_member_behind_box, 1, label);
    }

    /// The declaration placeholder of the sites file's `alias`.
    fn sites_placeholder(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        alias: &str,
    ) -> verter_type_engine::semantic_query::SemanticNodeId {
        dispatch.graph_arc().intern_node(SemanticNodeData::Opaque(
            verter_type_engine::semantic_query::QueryError::DeclPlaceholder {
                canonical_id: SITES_FILE.into(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: alias.into(),
                whole_hash: host
                    .shallow_file_state(SITES_FILE)
                    .expect("the declarations are indexed")
                    .whole_hash,
            },
        ))
    }

    /// The whole surface of `Plain | Other`, both declaration placeholders,
    /// in `Expanded`: the terminal's expansion instantiates each
    /// placeholder in turn, its union arms expanded on its worklist.
    fn expanded_placeholders(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> (SemanticQueryKey, Vec<SemanticQueryKey>) {
        let plain = sites_placeholder(host, dispatch, "Plain");
        let other = sites_placeholder(host, dispatch, "Other");
        let base = dispatch.graph_arc().intern_node(SemanticNodeData::Union(
            verter_type_engine::semantic_query::composite::CompositeList::test_fixture(Arc::from(
                [plain, other],
            )),
        ));
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let root = SemanticQueryKey::ProjectPath {
            base,
            path: member_path(&[]),
            context,
        };
        let reads = ["Plain", "Other"]
            .map(|alias| sites_instantiation(dispatch, alias, &[], context))
            .to_vec();
        (root, reads)
    }

    /// An `Expanded` terminal's expansion reads each declaration
    /// placeholder's instantiation as the walk's dependency, its worklist —
    /// the frames left and the arms already expanded — retained. Held at
    /// the first placeholder's instantiation or the second's (after the
    /// first arm expanded), the path yields; released, it resumes the
    /// worklist at the held frame, expanding nothing twice, to exactly what
    /// the synchronous entry reads.
    #[test]
    fn an_expansion_waiting_on_a_placeholders_instantiation_resumes_its_worklist() {
        let label = "expanded placeholders";
        assert_site_reads(&expanded_placeholders, label);
        for held in 0..2 {
            let at_hold = assert_site_suspends(&expanded_placeholders, held, label);
            assert_eq!(at_hold.hops, 0, "{label}: an empty path walks no hop");
            assert_site_releases(&expanded_placeholders, held, label);
        }
    }

    /// The sites host with a global `String` contributor indexed and then
    /// edited ([`edited_string_contributor_host`]): a walk reaching a
    /// `string` reads its wrapper's global lookup, which loads the pending
    /// contributor.
    fn edited_sites_host() -> Arc<crate::VerterHost> {
        let host = edited_string_contributor_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            SITES_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(SITES),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(SITES_FILE)
            .expect("the declarations have indexed input before a hop is held");
        host
    }

    /// `{ value: string }["value"]["marker"]` in `Navigate`, the object
    /// written in the sites file: the walk reads `marker` off the `string`
    /// member's apparent wrapper, the global `String`. The subject is a
    /// settled object, not an application: instantiating one resolves its
    /// declaration's augmentations, which indexes every pending
    /// contributor — the `String` one too — before the walk reaches the
    /// wrapper's lookup.
    fn string_member_of_object(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> SemanticQueryKey {
        use verter_type_engine::semantic_query::{SurfaceMember, SurfaceView};
        let string = dispatch
            .graph_arc()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let object = dispatch.graph_arc().intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView::from_members(
                vec![SurfaceMember {
                    key: verter_type_engine::semantic_query::AuthoredPropertyKey::string("value"),
                    value: string,
                    optional: false,
                    readonly: false,
                    method_kind: None,
                    has_implementation_body: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    spans: Default::default(),
                    declaration_origin: None,
                    declared_in_macro_type_arg:
                        verter_type_engine::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    merge_role: verter_type_engine::semantic_query::MergeRoleStamp::NEUTRAL,
                    excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                }],
                None,
            )),
            sites_scope(host),
        );
        SemanticQueryKey::ProjectPath {
            base: object,
            path: member_path(&["value", "marker"]),
            context: ProjectionReductionContext::published(ProjectionMode::Navigate),
        }
    }

    /// A walk reaching a wrapper mid-walk reads the wrapper's global lookup
    /// files as transitions of its own: with the `String` contributor's
    /// index cold after an edit and its shard held, the path returns
    /// `Pending` at the contributor's load — under a probe that forbids
    /// every wait, so it neither loads nor waits in place — holding the
    /// hops it walked. Released, it resumes the lookup at that load and
    /// delivers exactly what the synchronous entry reads on an equally
    /// edited host, each hop walked once.
    #[test]
    fn a_wrapper_lookup_reached_mid_walk_yields_at_its_contributors_load_and_resumes_there() {
        let (synchronous, synchronous_work) = {
            let host = edited_sites_host();
            let _request =
                verter_type_engine::request_context::install_test_request_for(SITES_FILE);
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let root = string_member_of_object(&host, &dispatch);
            let before = work(&host);
            let read = dispatch.execute_via_cold_build_helper(root);
            (read_summary(&dispatch, &read), work_since(&host, before))
        };
        assert!(
            synchronous.0.contains("str") && !synchronous.3,
            "premise: the contributed member, complete: {synchronous:?}"
        );

        let host = edited_sites_host();
        let _request = verter_type_engine::request_context::install_test_request_for(SITES_FILE);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let root = string_member_of_object(&host, &dispatch);
        let mut held = host.decl_lowering.hold_workers_for_tests();
        let before = work(&host);
        let mut entry = dispatch.poll_query(root);
        let counter = CountingThreadWaker::for_current_thread();
        let waker = Waker::from(Arc::clone(&counter));
        let first = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let at_hold = work_since(&host, before);
        let repolled = {
            let _forbid = verter_execution::blocking::WaitProbe::forbid();
            entry.poll(&waker).is_pending()
        };
        let repoll = work_since(&host, before);
        held.release();
        assert!(
            first,
            "the path yields while its wrapper's contributor indexes"
        );
        assert!(repolled, "a repoll while held enters no wait");
        assert_eq!(repoll, at_hold, "a pending repoll computes nothing");
        assert_eq!(
            at_hold.hops, 2,
            "the object's member and the wrapper's hop were walked: {at_hold:?}"
        );
        counter.wait_for_wake("the completed index wakes the waiting walk");
        let read = poll_until_ready(|waker| entry.poll(waker));
        drop(entry);
        assert_eq!(
            work_since(&host, before),
            synchronous_work,
            "no hop is walked again: the lookup resumes at its load"
        );
        assert_eq!(read_summary(&dispatch, &read), synchronous);
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        assert!(graph.resident_flight_keys_for_tests().is_empty());
        drop(scope);
    }

    const EXPLICIT_FILE: &str = "/semantic-task/explicit.ts";

    /// Generic functions whose instantiated signatures build template
    /// literals — one candidate with one template, one candidate with two,
    /// and two candidates — and a call naming the first with an explicit
    /// type argument.
    const EXPLICIT: &str = "declare function one<T extends string>(x: T): `prefix${T}`;\n\
                            declare function two<T extends string>(x: `a${T}`): `b${T}`;\n\
                            declare function many<T extends string>(x: T): `p${T}`;\n\
                            declare function many<T extends string, U = T>(x: T, y: U): `q${T}`;\n\
                            export const r = one<\"x\">(\"x\");\n";

    fn explicit_host() -> Arc<crate::VerterHost> {
        let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            EXPLICIT_FILE,
            &crate::u6_flow_shape_corpus_tests::module_script(EXPLICIT),
            crate::FileLanguage::script_ts(),
        );
        host.ensure_indexed_ready(EXPLICIT_FILE)
            .expect("the declarations have indexed input before a read is held");
        host
    }

    /// The value `name` the explicit file declares, read in `mode`.
    fn explicit_value(
        dispatch: &HostDispatch<'_>,
        name: &str,
        mode: ProjectionMode,
    ) -> SemanticQueryKey {
        dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    EXPLICIT_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: verter_type_expr::OwnedStr::from(name),
            },
            ProjectionReductionContext::published(mode),
        )
    }

    /// The template reduction a candidate's substitution of `"x"` for its
    /// clause parameter builds `head${T}` through: the template's one hole
    /// closed to the explicit argument.
    fn explicit_template(dispatch: &HostDispatch<'_>, head: &str) -> SemanticQueryKey {
        SemanticQueryKey::TemplateLiteralReduce {
            pattern: verter_type_expr::OwnedSlice::from(vec![
                verter_type_expr::OwnedStr::from(head),
                verter_type_expr::OwnedStr::from(""),
            ]),
            args: Arc::from([sites_literal(dispatch, "x")]),
            context: dispatch.template_literal_reduce_context(),
        }
    }

    /// The template reduction the call `one<"x">("x")` builds `prefix${T}`
    /// through: its overload set substitutes the call's own type argument,
    /// the literal `"x"` as the explicit file lowers it, scoped to that
    /// file.
    fn explicit_call_template(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
    ) -> SemanticQueryKey {
        let scope = verter_type_engine::semantic_query::NodeScopeId::File {
            canonical_id: EXPLICIT_FILE.into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: host
                .shallow_file_state(EXPLICIT_FILE)
                .expect("the declarations are indexed")
                .whole_hash,
            local_scope: None,
        };
        let argument = dispatch.graph_arc().intern_node_with_scope(
            SemanticNodeData::Literal(verter_type_engine::semantic_query::LiteralValue::String(
                "x".into(),
            )),
            scope,
        );
        SemanticQueryKey::TemplateLiteralReduce {
            pattern: verter_type_expr::OwnedSlice::from(vec![
                verter_type_expr::OwnedStr::from("prefix"),
                verter_type_expr::OwnedStr::from(""),
            ]),
            args: Arc::from([argument]),
            context: dispatch.template_literal_reduce_context(),
        }
    }

    /// The overload set of `name`'s call signatures under the explicit type
    /// argument `"x"`, and the reads its build names: the callee's
    /// signature list, then the template reduction each head in `heads`
    /// builds through, in order.
    fn explicit_set(
        host: &crate::VerterHost,
        dispatch: &HostDispatch<'_>,
        name: &str,
        heads: &[&str],
    ) -> SiteRead {
        let value = dispatch.execute_read(explicit_value(dispatch, name, ProjectionMode::Expanded));
        let QueryResult::Value(callee) = value.value() else {
            panic!("`{name}` has a type: {value:?}");
        };
        let kind = verter_type_engine::semantic_query::SignatureKind::Call;
        let set = SemanticQueryKey::ResolveOverloadSet {
            callee: *callee,
            kind,
            type_args: Arc::from([sites_literal(dispatch, "x")]),
            context: verter_type_engine::semantic_query::OverloadSetContext {
                resolve_env_hash: host
                    .host_view_env_hashes_for(EXPLICIT_FILE)
                    .resolve_env_hash,
                ..Default::default()
            },
        };
        let mut reads = vec![SemanticQueryKey::SignaturesOfType {
            subject: *callee,
            kind,
            context: verter_type_engine::semantic_query::SemanticContextId::production(),
        }];
        reads.extend(heads.iter().map(|head| explicit_template(dispatch, head)));
        (set, reads)
    }

    /// `one<"x">`: one candidate, whose return template reads one reduction.
    fn explicit_one(host: &crate::VerterHost, dispatch: &HostDispatch<'_>) -> SiteRead {
        explicit_set(host, dispatch, "one", &["prefix"])
    }

    /// `two<"x">`: one candidate, whose parameter and return templates each
    /// read a reduction.
    fn explicit_two(host: &crate::VerterHost, dispatch: &HostDispatch<'_>) -> SiteRead {
        explicit_set(host, dispatch, "two", &["a", "b"])
    }

    /// `many<"x">`: two candidates, each with a return template; the second
    /// fills its defaulted parameter from the first.
    fn explicit_many(host: &crate::VerterHost, dispatch: &HostDispatch<'_>) -> SiteRead {
        explicit_set(host, dispatch, "many", &["p", "q"])
    }

    /// The explicit-argument part of a read's work: the candidates its sets
    /// began instantiating, the substitutions their clauses began, and the
    /// children its substitution walks visited.
    fn explicit_work(work: Work) -> (u64, u64, u64) {
        (work.candidates, work.binders, work.visits)
    }

    /// The template reductions an explicit overload set's candidate
    /// substitutions read: its only one, the first and the later one inside
    /// one candidate, and a later candidate's.
    const EXPLICIT_SEAMS: [HeldSeam; 4] = [
        HeldSeam {
            seam: "an explicit overload set's template reduction",
            host: explicit_host,
            keys: |host, dispatch| {
                let (set, reads) = explicit_one(host, dispatch);
                (set, reads[1].clone())
            },
            expression_site: true,
            // The set begins and reads its callee's list before the wait.
            before_seam: (1, 1),
            // The reduction begins its own build; the set resumes once with
            // it.
            in_all: (2, 2),
        },
        HeldSeam {
            seam: "an explicit candidate's first template reduction",
            host: explicit_host,
            keys: |host, dispatch| {
                let (set, reads) = explicit_two(host, dispatch);
                (set, reads[1].clone())
            },
            expression_site: true,
            before_seam: (1, 1),
            // Each reduction begins its own build; the set resumes with
            // each.
            in_all: (3, 3),
        },
        HeldSeam {
            seam: "an explicit candidate's later template reduction",
            host: explicit_host,
            keys: |host, dispatch| {
                let (set, reads) = explicit_two(host, dispatch);
                (set, reads[2].clone())
            },
            expression_site: true,
            // The parameter's reduction is built and taken before the wait.
            before_seam: (2, 2),
            in_all: (3, 3),
        },
        HeldSeam {
            seam: "a later explicit candidate's template reduction",
            host: explicit_host,
            keys: |host, dispatch| {
                let (set, reads) = explicit_many(host, dispatch);
                (set, reads[2].clone())
            },
            expression_site: true,
            // The first candidate's reduction is built and taken before the
            // wait.
            before_seam: (2, 2),
            in_all: (3, 3),
        },
    ];

    /// An overload set with an explicit type argument instantiates its
    /// candidates as a continuation. At a template reduction a candidate's
    /// substitution reads that another task is producing — the set's only
    /// one, the first or a later one inside one candidate, or a later
    /// candidate's — the polled set yields without joining the producer in
    /// place; a repoll that forbids every wait stays pending and computes
    /// nothing, and so does its task polled on another thread. Released, it
    /// resumes there from what it retained and reads exactly what the
    /// synchronous entry reads, receipt tree included: no candidate, binder
    /// substitution or walk child is computed again, and the synchronous
    /// entry itself computes each once.
    #[test]
    fn an_explicit_overload_set_suspends_at_each_template_reduction_and_resumes_to_the_synchronous_read(
    ) {
        let label = "an explicit set's only template reduction";
        let reads = assert_site_reads_on(explicit_host, &explicit_one, label);
        assert_eq!(
            reads.len(),
            2,
            "{label}: premise: the set names its callee's list and one reduction: {reads:#?}"
        );
        let (at_hold, in_all) =
            assert_site_suspends_on(explicit_host, &explicit_one, 1, true, label);
        assert_eq!(
            explicit_work(at_hold),
            (1, 1, 3),
            "{label}: the candidate began, its binder substitution began, and the parameter, the return and its hole were visited before the wait"
        );
        assert_eq!(
            explicit_work(in_all),
            (1, 1, 4),
            "{label}: each is computed once, the clause constraint visited after the reduction"
        );

        let label = "an explicit candidate's two template reductions";
        let reads = assert_site_reads_on(explicit_host, &explicit_two, label);
        assert_eq!(
            reads.len(),
            3,
            "{label}: premise: the set names its callee's list and two reductions: {reads:#?}"
        );
        let (first, in_all) = assert_site_suspends_on(explicit_host, &explicit_two, 1, true, label);
        assert_eq!(
            explicit_work(first),
            (1, 1, 2),
            "{label}: the parameter template and its hole were visited before the first wait"
        );
        assert_eq!(
            explicit_work(in_all),
            (1, 1, 5),
            "{label}: the parameter, the return, both holes and the constraint are each visited once"
        );
        let (later, _) = assert_site_suspends_on(explicit_host, &explicit_two, 2, true, label);
        assert_eq!(
            explicit_work(later),
            (1, 1, 4),
            "{label}: the parameter's reduction was taken and the return's hole visited before the later wait"
        );

        let label = "a later explicit candidate's template reduction";
        let reads = assert_site_reads_on(explicit_host, &explicit_many, label);
        assert_eq!(
            reads.len(),
            3,
            "{label}: premise: the set names its callee's list and one reduction per candidate: {reads:#?}"
        );
        let (at_hold, in_all) =
            assert_site_suspends_on(explicit_host, &explicit_many, 2, true, label);
        assert_eq!(
            (at_hold.candidates, at_hold.binders),
            (2, 2),
            "{label}: the first candidate finished and the second began its first binder before the wait"
        );
        assert_eq!(
            (in_all.candidates, in_all.binders),
            (2, 4),
            "{label}: the second candidate's default and its own binder follow the wait, each once"
        );
        assert!(
            at_hold.visits < in_all.visits,
            "{label}: the second candidate's walk continues after the wait: {at_hold:?} {in_all:?}"
        );
    }

    /// At each template reduction an explicit overload set's candidates
    /// read, the polled set runs exactly the stages before the reduction,
    /// waits in one frame, resumes elsewhere at the stage it retained, and
    /// reads what the synchronous entry reads; warm, both entries answer
    /// alike running no stage.
    #[test]
    fn an_explicit_overload_set_resumes_each_template_reduction_at_the_stage_it_retained() {
        for seam in &EXPLICIT_SEAMS {
            assert_seam_suspends_and_resumes(seam);
        }
    }

    /// An explicit overload set released, or stopped by its request's
    /// cancellation, while a candidate stands at a template reduction leaves
    /// nothing open — no cost recording its substitution walk opened, no
    /// wait, no flight but the holders' — and the dispatcher then reads it
    /// as the synchronous entry does.
    #[test]
    fn an_explicit_set_released_or_cancelled_at_a_template_reduction_leaves_nothing_open() {
        for seam in &EXPLICIT_SEAMS {
            assert_seam_releases(seam);
        }
    }

    /// A call with an explicit type argument whose overload set reads a
    /// template reduction another task is producing yields instead of
    /// blocking: the call's walk requests the set, which the runtime builds
    /// as a continuation standing at the reduction. Released, it resumes to
    /// exactly what the synchronous entry reads. The set instantiates the
    /// call's one candidate once, and the call instantiates again only the
    /// candidate it selects: the raw candidates it pairs back to are
    /// filtered without instantiating any.
    #[test]
    fn a_call_with_explicit_type_arguments_yields_while_its_overload_set_waits() {
        let label = "a call with an explicit type argument";
        let (synchronous, synchronous_work) = {
            let host = explicit_host();
            // The call is demanded by a request on its own file, as a root a
            // request reads from its file is.
            let _request =
                verter_type_engine::request_context::install_test_request_for(EXPLICIT_FILE);
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let _scope = dispatch.enter_construction_scope();
            let call = explicit_value(&dispatch, "r", ProjectionMode::Navigate);
            let before = work(&host);
            let read = dispatch.execute_via_cold_build_helper(call);
            let work = work_since(&host, before);
            assert_eq!(
                host.project_type_store()
                    .semantic_graph()
                    .slot_candidate_count_for_tests(&explicit_call_template(&host, &dispatch)),
                1,
                "{label}: premise: the call's overload set reads this template reduction"
            );
            (read_summary(&dispatch, &read), work)
        };
        assert!(
            !synchronous.3 && synchronous.5.is_some(),
            "{label}: premise: a complete, priced answer: {synchronous:?}"
        );
        assert_eq!(
            synchronous_work.candidates, 1,
            "{label}: the set instantiates the call's one candidate once"
        );
        assert_eq!(
            synchronous_work.binders, 2,
            "{label}: the set's binder substitution and the selected candidate's; 3 means the raw pairing instantiated its candidate"
        );

        let host = explicit_host();
        let _request = verter_type_engine::request_context::install_test_request_for(EXPLICIT_FILE);
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
        let call = explicit_value(&dispatch, "r", ProjectionMode::Navigate);
        let held = explicit_call_template(&host, &dispatch);
        let (tasks, leases) = hold_seam(&host, graph, held, true);
        let counter = Arc::new(CountingWaker::default());
        let waker = Waker::from(Arc::clone(&counter));
        let before = work(&host);
        let mut entry = dispatch.poll_query(call);
        let (first, first_waits, pending_elsewhere) = while_held(leases, || {
            let (first, first_waits) = {
                let probe = verter_execution::blocking::WaitProbe::record();
                let first = entry.poll(&waker);
                (first, probe.reached())
            };
            let pending_elsewhere = first.is_pending().then(|| {
                entry.task().map(|task| {
                    assert_send(task);
                    std::thread::scope(|scope| {
                        scope
                            .spawn(|| task.poll(&waker).is_pending())
                            .join()
                            .expect("the pending poll returns")
                    })
                })
            });
            (first, first_waits, pending_elsewhere.flatten())
        });
        let woken = counter.0.load(std::sync::atomic::Ordering::SeqCst) >= 1;
        let yielded = first.is_pending();
        let read = match first {
            Poll::Ready(read) => read,
            Poll::Pending => resume_elsewhere(&mut entry),
        };
        drop(entry);
        let total = work_since(&host, before);
        assert!(
            first_waits
                .iter()
                .all(|family| *family == verter_execution::blocking::WaitFamily::DeclLowering),
            "{label}: the first poll joins no dependency in place: {first_waits:?}"
        );
        assert!(
            yielded,
            "{label}: the call yields while its set's template reduction is held"
        );
        assert_eq!(pending_elsewhere, Some(true), "{label}");
        assert!(woken, "{label}: the producer going away wakes the call");
        assert_eq!(
            total, synchronous_work,
            "{label}: nothing completed before the wait is computed again"
        );
        assert_eq!(
            dispatch.open_request_state_for_tests(),
            NOTHING_OPEN,
            "{label}"
        );
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0, "{label}");
        drop(tasks);
        drop(scope);
        drop(dispatch);
        let dispatch = ProjectSemanticDispatch::new(&*host);
        assert_eq!(read_summary(&dispatch, &read), synchronous, "{label}");
    }
}

/// How the subscriber to a held semantic flight is driven through the
/// root-continuation harness.
#[derive(Clone, Copy)]
enum FollowerDrive {
    /// The pollable subscription.
    Poll,
    /// The synchronous wait: the planted blocking wait the probe refuses.
    SynchronousWait,
    /// A park the probe cannot see: the planted blocking wait only the
    /// deadline catches.
    UnannouncedPark,
}

/// Hold another task's semantic flight and drive a second task's
/// subscription to it as `drive` says, through the root-continuation
/// harness. Whatever the verdict, no wait edge or task is left behind.
fn drive_semantic_follower(
    drive: FollowerDrive,
    deadline: std::time::Duration,
) -> Result<bool, crate::root_continuations_tests::NotSuspended> {
    use verter_type_engine::semantic_query_memo::Joined;

    let host = chain_host();
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let key = typeof_key(&dispatch, "x0");
    let graph = host.project_type_store().semantic_graph();
    let (producer_task, lease) = hold_producer(&host, graph, key.clone());
    let follower_task = graph.task_registry_for_tests().register_task();
    let mut capture = ReadCapture::default();
    let mut claim = graph
        .begin_query_claim(&*host, host.request_flags(), key, &mut capture)
        .unwrap_or_else(|_| panic!("the flight is held"));
    let Claim::Subscribed(subscription) = graph.claim_query(
        &*host,
        host.request_flags(),
        &mut claim,
        &follower_task,
        &mut capture,
    ) else {
        panic!("the follower subscribes to the held flight");
    };
    let parked = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));
    let release_park = Arc::clone(&parked);
    let host_ref = &*host;
    let verdict = crate::root_continuations_tests::pending_before_release(
        deadline,
        (Some(subscription), claim, capture),
        move |(subscription, claim, capture), waker| match drive {
            FollowerDrive::Poll => subscription
                .as_mut()
                .expect("polled until its answer")
                .poll(host_ref, host_ref.request_flags(), claim, capture, waker)
                .map(|joined| matches!(joined, Joined::Retry | Joined::Read(_))),
            FollowerDrive::SynchronousWait => {
                let joined = subscription.take().expect("waited once").wait(
                    host_ref,
                    host_ref.request_flags(),
                    claim,
                    capture,
                );
                Poll::Ready(matches!(joined, Joined::Retry | Joined::Read(_)))
            }
            FollowerDrive::UnannouncedPark => {
                let (released, changed) = &*parked;
                let mut released = released.lock();
                while !*released {
                    changed.wait(&mut released);
                }
                Poll::Ready(true)
            }
        },
        move || {
            let (released, changed) = &*release_park;
            *released.lock() = true;
            changed.notify_all();
            drop(lease);
        },
    );
    drop((producer_task, follower_task));
    assert_eq!(
        graph.wait_graph_counts_for_tests(),
        (0, 0),
        "the harness leaves no wait edge or task open",
    );
    verdict
}

/// The pollable subscriber of a held semantic flight returns `Pending`
/// before the producer releases it, is woken by the release, and answers.
#[test]
fn root_continuations_harness_passes_a_suspending_flight_subscriber() {
    assert_eq!(
        drive_semantic_follower(
            FollowerDrive::Poll,
            crate::root_continuations_tests::POLL_DEADLINE
        ),
        Ok(true)
    );
}

/// Planted blocking wait, announced: the synchronous form of the same
/// subscription fails the harness at once, naming its family, and the
/// harness still releases the producer and joins its thread.
#[test]
fn root_continuations_harness_fails_a_planted_synchronous_wait() {
    match drive_semantic_follower(
        FollowerDrive::SynchronousWait,
        crate::root_continuations_tests::POLL_DEADLINE,
    ) {
        Err(crate::root_continuations_tests::NotSuspended::EnteredWait(message)) => assert!(
            message.contains("SemanticFlight"),
            "the refusal names the family: {message}",
        ),
        other => panic!("a synchronous wait cannot pass for a suspension: {other:?}"),
    }
}

/// Planted blocking wait, unannounced: a park the probe cannot see fails
/// the harness at its injected deadline, and releasing the hold lets the
/// parked thread finish, so the harness joins it and returns.
#[test]
fn root_continuations_harness_fails_a_planted_unannounced_park_at_the_deadline() {
    let started = std::time::Instant::now();
    assert_eq!(
        drive_semantic_follower(
            FollowerDrive::UnannouncedPark,
            crate::root_continuations_tests::PLANTED_DEADLINE
        ),
        Err(crate::root_continuations_tests::NotSuspended::Blocked),
    );
    assert!(
        started.elapsed() < crate::root_continuations_tests::POLL_DEADLINE,
        "the harness returns soon after its injected deadline",
    );
}

/// A storage-only narrow fact policy: a set past 16 facts (8 when built by
/// extension) is a tree, while the strict-root and precise-domain decisions
/// keep their production widths, so a host under it answers and seals the
/// same facts a production host does, in different storage.
fn narrow_storage_policy() -> verter_session_query::facts::policy::FactPolicy {
    verter_session_query::facts::policy::FactPolicy::with_widths(
        verter_session_query::facts::policy::FactWidths {
            fresh_slice_max: 16,
            strict_root_threshold: 1_024,
            precise_domain_max: 1_024,
            extension_slice_max: 8,
        },
    )
    .expect("valid widths")
}

/// [`crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host`]
/// composed with `policy`: the workspace seals its resolution witnesses
/// under it and the host builds every set under it.
fn audit_host_under(
    policy: verter_session_query::facts::policy::FactPolicy,
) -> Arc<crate::VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new_with_fact_policy(
        verter_workspace::MemoryOptions::default(),
        verter_session_query::resolution::InputResolutionBudgets::default(),
        policy,
    ));
    Arc::new(crate::VerterHost::new_with_scheduler_config(
        crate::HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: false,
            ..crate::HostConfig::default()
        },
        workspace,
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    ))
}

/// A task suspended on another producer and resumed on another thread
/// seals what it publishes under the policy of the request that lent it
/// its state: the policy travels with the task, never with the thread.
#[test]
fn a_task_resumed_on_another_thread_seals_under_its_hosts_fact_policy() {
    let policy = narrow_storage_policy();
    let host = audit_host_under(policy);
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        TASK_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(CHAIN),
        crate::FileLanguage::script_ts(),
    );
    assert_eq!(host.project_type_store().fact_policy(), policy);
    let dispatch = ProjectSemanticDispatch::new(&*host);
    let answer = drive_x5_across_a_wait(&host, &dispatch, true);
    assert_eq!(display(&dispatch, &answer), "\"ok\"");
    let graph = host.project_type_store().semantic_graph();
    let mut sealed = 0;
    for name in ["x5", "x4", "x3", "x2"] {
        if let Some(signature) =
            graph.entry_read_set_signature_for_tests(&typeof_key(&dispatch, name))
        {
            assert_eq!(
                signature.facts.policy(),
                policy,
                "the entry for `typeof {name}` is sealed under the host's policy"
            );
            sealed += 1;
        }
    }
    assert!(sealed > 0, "the resumed task publishes what it produced");
}

/// The `typeof v0` of a chain of `count` modules, each constant initialized
/// from the next module's: its read set names every module of the chain.
fn module_chain_host(
    policy: verter_session_query::facts::policy::FactPolicy,
    count: usize,
) -> Arc<crate::VerterHost> {
    let host = audit_host_under(policy);
    for index in 0..count {
        let source = if index + 1 == count {
            format!("export const v{index} = \"ok\" as const;")
        } else {
            let next = index + 1;
            format!("import {{ v{next} }} from \"./m{next}\";\nexport const v{index} = v{next};")
        };
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            &format!("/fact-policy/m{index}.ts"),
            &source,
            crate::FileLanguage::script_ts(),
        );
    }
    host
}

/// The answer and the sealed read set of a traced cold `typeof v0`.
fn traced_chain_head(
    host: &crate::VerterHost,
) -> (String, verter_session_query::facts::fact_set::FactSet) {
    let dispatch = ProjectSemanticDispatch::new(host);
    let key = dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(
                "/fact-policy/m0.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: verter_type_expr::OwnedStr::from("v0"),
        },
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    );
    let (answer, read_set) = host.with_fact_tracer(
        verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
        || {
            let read = dispatch.execute_read(key);
            match read.value() {
                QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(*node)),
                other => panic!("the chain head answers a type: {other:?}"),
            }
        },
    );
    let verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(set) =
        read_set.finalise()
    else {
        panic!("a stable, cacheable chain seals its read set");
    };
    (display(&dispatch, &answer), set)
}

/// Two hosts composed with different fact policies run the same workload
/// at once in one process. Each seals every set under its own policy —
/// the narrow host's wide read set is a charged tree — and both seal the
/// same facts and give the same answer: a policy chooses storage, never
/// content, and never leaks from one host into the other.
#[test]
fn hosts_with_different_fact_policies_run_side_by_side() {
    const MODULES: usize = 24;
    let narrow = narrow_storage_policy();
    let production = verter_session_query::facts::policy::FactPolicy::production();
    let ((production_answer, production_set), (narrow_answer, narrow_set)) =
        std::thread::scope(|scope| {
            let production_run =
                scope.spawn(|| traced_chain_head(&module_chain_host(production, MODULES)));
            let narrow_run = scope.spawn(|| traced_chain_head(&module_chain_host(narrow, MODULES)));
            (
                production_run.join().expect("the production host answers"),
                narrow_run.join().expect("the narrow host answers"),
            )
        });
    assert_eq!(production_answer, "\"ok\"");
    assert_eq!(narrow_answer, production_answer);
    // A resolution fact names the population of the workspace engine that
    // decided it, and every engine mints its own: the two hosts agree on
    // every other fact and on how many resolutions each decided.
    let (narrow_resolutions, narrow_facts): (Vec<_>, Vec<_>) =
        narrow_set.to_vec().into_iter().partition(|fact| {
            matches!(
                fact,
                verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(_)
            )
        });
    let (production_resolutions, production_facts): (Vec<_>, Vec<_>) =
        production_set.to_vec().into_iter().partition(|fact| {
            matches!(
                fact,
                verter_session_query::facts::fact_cache::FactVersionRef::ResolveImports(_)
            )
        });
    assert_eq!(
        narrow_facts, production_facts,
        "both hosts seal the same facts"
    );
    assert!(!narrow_resolutions.is_empty());
    assert_eq!(narrow_resolutions.len(), production_resolutions.len());
    assert!(
        narrow_set.len() > narrow.fresh_slice_max(),
        "the read set is wide under the narrow policy: {}",
        narrow_set.len()
    );
    assert_eq!(production_set.policy(), production);
    assert_eq!(narrow_set.policy(), narrow);
    assert!(
        narrow_set.tree_storage().is_some() && narrow_set.tree_node_charges() > 0,
        "under the narrow policy the wide set is a tree charging its own nodes"
    );
    println!(
        "read set: {} facts; production storage flat: {}",
        production_set.len(),
        production_set.as_flat().is_some()
    );
}

/// A host's policy is fixed when it is composed: binding a workspace that
/// seals its witnesses under another policy is refused.
#[test]
#[should_panic(expected = "different fact policies")]
fn a_workspace_of_another_fact_policy_cannot_be_bound() {
    let host = audit_host_under(narrow_storage_policy());
    host.set_workspace(Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    )));
}
