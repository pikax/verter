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
    ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId, SemanticQueryKey,
    SemanticQueryValue, ValueRootKey,
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

const NOTHING_OPEN: OpenRequestStateForTests = OpenRequestStateForTests {
    active_instantiations: 0,
    open_cost_scopes: 0,
    request_local_open: 0,
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
