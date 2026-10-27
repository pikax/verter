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
    let waker = Waker::from(Arc::new(CountingWaker::default()));
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
    let other = graph.task_registry_for_tests().register_task();
    let mut capture = ReadCapture::default();
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
