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

/// What a read answered, in its own value domain, its fact status, its
/// rails, its diagnostics and its receipt's logical work: the parts of a
/// read a consumer of a root query decides on.
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
            "{:?}",
            (
                receipt.exclusive(),
                receipt.closure_usage(),
                receipt.footprint(),
                receipt.depth(),
                receipt.prerequisites().len(),
            )
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
