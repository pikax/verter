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
fn hold_producer<'g>(
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
            let mut finish = || match task.poll_for_tests(&waker) {
                Poll::Ready(TaskOutcomeForTests::Answered(answer)) => answer,
                Poll::Ready(TaskOutcomeForTests::Stopped) => panic!("nothing stops the task"),
                Poll::Pending => panic!("the woken task produces x2 itself"),
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
    use super::*;

    // Query-seam fixtures start with indexed input; the source/index hold
    // fixtures below exercise those earlier suspension boundaries separately.
    fn chain_host() -> Arc<crate::VerterHost> {
        let host = super::chain_host();
        host.ensure_indexed_ready(TASK_FILE)
            .expect("the query-seam fixture has indexed input");
        host
    }

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

    type JoinedEvidence = (
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
    fn joined_evidence(
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
    fn assert_joined_matches(joined: JoinedEvidence, synchronous: &JoinedEvidence) {
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
    #[ignore = "a flow frame drops its evaluation at an unanswered read and evaluates the body again after the delivery: its evaluator positions are not yet retained across a suspension"]
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

    /// `key` polled at the production entry on a fresh `host` while the
    /// signature kernel replaces its epoch exactly once, at the first time a
    /// list reaches `point` — as its build settles the list it publishes, or
    /// as a consumer about to pin the completed list holds it — against
    /// what the synchronous entry reads on a fresh host with no replacement.
    fn read_across_one_retirement(
        point: verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint,
        host: fn() -> Arc<crate::VerterHost>,
        key: fn(
            &crate::VerterHost,
            &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
        ) -> SemanticQueryKey,
    ) -> (ReadDecision, ReadDecision, (u64, u64)) {
        let synchronous = {
            let host = host();
            let dispatch = ProjectSemanticDispatch::new(&*host);
            let key = key(&host, &dispatch);
            decision(read_summary(
                &dispatch,
                &dispatch.execute_via_cold_build_helper(key),
            ))
        };
        let host = host();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let key = key(&host, &dispatch);
        let graph = Arc::clone(dispatch.graph_arc());
        let replacements = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let before = continuation_stages(&host);
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
            "premise: exactly one list is retired at {point:?}"
        );
        let after = continuation_stages(&host);
        let polled = decision(read_summary(&dispatch, &read));
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        (
            polled,
            synchronous,
            (after.0 - before.0, after.1 - before.1),
        )
    }

    /// A signature list the kernel retired is no answer to a need of it:
    /// retired as its build settles it, the request never keeps it; retired
    /// after the request completed it — as its consumer holds it to pin —
    /// the consumer that meets the retired list reads it once more, and that
    /// read recomputes it in the current epoch instead of being handed the
    /// completed retired list again. An overload set over authored leaves,
    /// one over a composite candidate and an apparent type whose anchor list
    /// is retired each read exactly what the synchronous entry reads with no
    /// replacement, each reading the retired list exactly once more.
    ///
    /// Handed the completed retired list again, the consumer's one re-read
    /// meets the same retired handles and answers `Error(Miss)`.
    #[test]
    fn completed_signature_list_is_revalidated_after_retirement() {
        use verter_type_engine::project_semantic_dispatch::signature_discovery::SignaturesOfTypeBuildPoint;
        for point in [
            SignaturesOfTypeBuildPoint::Settled,
            SignaturesOfTypeBuildPoint::Pinning,
        ] {
            let overloads = read_across_one_retirement(point, contextual_host, |host, dispatch| {
                overload_set_keys(host, dispatch).0
            });
            assert!(
                overloads.1 .0.contains("OverloadSet"),
                "premise: {overloads:?}"
            );
            assert_eq!(
                overloads.0, overloads.1,
                "{point:?}: an overload set over authored leaves"
            );
            assert_eq!(
                overloads.2,
                (1, 2),
                "{point:?}: the set reads its list, and once more after the retirement"
            );

            let composite =
                read_across_one_retirement(point, union_callee_host, |host, dispatch| {
                    union_overload_set_keys(host, dispatch).0
                });
            assert!(
                composite.1 .0.contains("OverloadSet") && !composite.1 .3,
                "premise: {composite:?}"
            );
            assert_eq!(
                composite.0, composite.1,
                "{point:?}: an overload set over a composite"
            );
            assert_eq!(
                composite.2,
                (1, 3),
                "{point:?}: the list, once more after the retirement, then the composite's result"
            );

            let apparent =
                read_across_one_retirement(point, apparent_callee_host, |_, dispatch| {
                    apparent_type_keys(dispatch).0
                });
            assert!(apparent.1 .0.contains("marker"), "premise: {apparent:?}");
            assert_eq!(apparent.0, apparent.1, "{point:?}: an apparent type");
            assert_eq!(
                apparent.2,
                (1, 7),
                "{point:?}: the apparent type's six reads, and its anchor list once more after the retirement"
            );
        }
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
                (1, 6),
                "the apparent type begins once, then reads its anchor list, its call and construct lists for the strict interface, looks up `CallableFunction` and then `Function`, and reads the interface, each once"
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
            (1, 6),
            "no stage runs again: every list, lookup and the interface are each read once"
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
    fn poll_until_ready<T>(mut poll: impl FnMut(&Waker) -> Poll<T>) -> T {
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
        let cases: [(&str, usize, Key); 5] = [
            ("a declaration's source", 1, |_, _| {
                declaration_key(MISSING_OWNER)
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
    const HELD_SEAMS: [HeldSeam; 10] = [
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
            seam: "an apparent type's anchor list",
            host: apparent_callee_host,
            keys: |_, dispatch| apparent_type_keys(dispatch),
            expression_site: true,
            before_seam: (1, 0),
            // The anchor list, the call and construct lists, the
            // `CallableFunction` and `Function` lookups and the interface.
            in_all: (1, 6),
        },
        HeldSeam {
            seam: "an apparent type's construct list",
            host: apparent_callee_host,
            keys: apparent_construct_list_keys,
            expression_site: true,
            // The anchor list and the call list are read before the wait.
            before_seam: (1, 2),
            in_all: (1, 6),
        },
        HeldSeam {
            seam: "an apparent type's interface instantiation",
            host: apparent_callee_host,
            keys: apparent_interface_keys,
            expression_site: true,
            // Every list and both lookups are made before the wait.
            before_seam: (1, 5),
            in_all: (1, 6),
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
    }

    /// At every suspension seam, a root released while it waits — dropped
    /// by its consumer, or stopped by its request's cancellation, which wakes
    /// it with the producer still held — gives its dispatcher back with
    /// nothing open, no wait and no flight but the holding producers', and
    /// the dispatcher then reads the root as the synchronous entry does.
    #[test]
    fn a_root_released_or_cancelled_at_any_converted_seam_leaves_nothing_open() {
        for seam in &HELD_SEAMS {
            let (synchronous, _) = synchronous_across(seam);
            for cancelled in [false, true] {
                let host = (seam.host)();
                let mut dispatch = ProjectSemanticDispatch::new(&*host);
                let _scope = dispatch.enter_construction_scope();
                let graph = host.project_type_store().semantic_graph();
                let (root, held) = (seam.keys)(&host, &dispatch);
                let (tasks, leases) = hold_seam(&host, graph, held, seam.expression_site);
                let counter = Arc::new(CountingWaker::default());
                let waker = Waker::from(Arc::clone(&counter));
                let (pending, stopped, flights) = while_held(leases, || {
                    if cancelled {
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
    }

    /// An overload set whose signature list the kernel retires while another
    /// task holds the result its composite candidate reads: once released,
    /// that result cannot be read for a descriptor of the retired epoch, so
    /// the set — its first stage never run again — reads its list once more,
    /// in the current epoch, and its composite's result after it, then
    /// answers exactly what the synchronous entry answers with no
    /// retirement, leaving nothing open.
    ///
    /// Taking the failed result read as an unsettled list instead, the set
    /// answers `Error(Miss)` for a valid overload set.
    #[test]
    fn an_overload_set_whose_list_retires_while_its_composite_result_is_held_reads_its_list_once_more(
    ) {
        let seam = &HELD_SEAMS[5];
        let (synchronous, _) = synchronous_across(seam);
        let host = (seam.host)();
        let mut dispatch = ProjectSemanticDispatch::new(&*host);
        let _scope = dispatch.enter_construction_scope();
        let graph = host.project_type_store().semantic_graph();
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
            (1, 4),
            "the set begins once and reads its list, its retired composite's result, its list again and its composite's result again, each once"
        );
        assert_eq!(
            decision(read_summary(&dispatch, &read)),
            decision(synchronous)
        );
        assert_eq!(dispatch.open_request_state_for_tests(), NOTHING_OPEN);
        assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
        drop(tasks);
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
