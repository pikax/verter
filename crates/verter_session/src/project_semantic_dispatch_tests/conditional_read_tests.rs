use std::sync::Arc;
use std::task::{Poll, Waker};

use verter_type_engine::semantic_query::{
    PrimitiveKind, QueryResult, SemanticNodeData, SemanticQueryKey, SemanticQueryValue,
};

use super::carrier_head_resolution_tests::host;
use super::semantic_task_tests::hold_producer;
use super::ProjectSemanticDispatch;

use verter_type_engine::project_semantic_dispatch::ConditionalProgressForTests;
use verter_type_engine::request_context::{
    current_cold_compute_completeness, ColdComputeCompletenessScope,
};
use verter_type_engine::semantic_query::{
    DeclIdentity, IndexKey, IndexedAccessOrigin, LiteralValue, PayloadArray, SemanticNodeId,
    SemanticQueryKeyTag, TupleElement,
};
use verter_type_expr::{OwnedSlice, OwnedStr, TopLevelOwnerId};

const CASES: &[&str] = &[
    "tuple",
    "alias",
    "declared",
    "indexed",
    "keyof",
    "template",
    "nested",
    "written",
    "infer",
    "both",
    "distribution",
];
const FILE: &str = "/conditional-read.ts";

fn fixture(
    host: &crate::VerterHost,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    case: &str,
) -> (SemanticQueryKey, bool) {
    let graph = dispatch.graph_arc();
    let primitive = |kind| graph.intern_node(SemanticNodeData::Primitive(kind));
    let number = primitive(PrimitiveKind::Number);
    let string = primitive(PrimitiveKind::String);
    let any = primitive(PrimitiveKind::Any);
    let literal = |value| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(value)));
    let one = literal(1.0);
    let two = literal(2.0);
    let identity = |name: &str| DeclIdentity {
        canonical_id: FILE.into(),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: host
            .shallow_file_state(FILE)
            .expect("indexed declarations")
            .whole_hash,
        decl_name: name.into(),
    };
    let named = |name| {
        graph.intern_node(SemanticNodeData::DeclRef {
            identity: identity(name),
        })
    };
    let tuple = |values: &[SemanticNodeId]| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: PayloadArray::from(
                values
                    .iter()
                    .map(|&value| TupleElement {
                        label: None,
                        value,
                        optional: false,
                        rest: false,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: false,
        })
    };
    let union = |values: &[SemanticNodeId]| {
        graph.intern_node(SemanticNodeData::Union(
            verter_type_engine::semantic_query::composite::CompositeList::test_fixture(Arc::from(
                values,
            )),
        ))
    };
    let template = || {
        graph.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: OwnedSlice::from(
                vec![OwnedStr::from(""), OwnedStr::from("")].into_boxed_slice(),
            ),
            expressions: PayloadArray::from(vec![number]),
        })
    };
    let binder = graph.alloc_infer_binder_id();
    let infer = graph.intern_node(SemanticNodeData::Infer {
        name: "U".into(),
        binder: binder.clone(),
        constraint: None,
    });
    let reference = graph.intern_node(SemanticNodeData::InferRef {
        name: "U".into(),
        binder,
    });
    let (check, extends, true_branch, false_branch, distributive) = match case {
        "alias" => {
            let param = graph.intern_node(SemanticNodeData::TypeParam {
                decl: identity("Open"),
                param_index: 0,
                constraint: None,
                default: None,
                display_name: "T".into(),
            });
            let application = graph.intern_node(SemanticNodeData::InstantiationRef {
                base: identity("Open"),
                args: PayloadArray::from(vec![param]),
            });
            (application, number, one, two, false)
        }
        "tuple" => (tuple(&[template()]), tuple(&[string]), one, two, false),
        "declared" => (named("A"), number, one, two, false),
        "indexed" => {
            let check = graph.intern_node(SemanticNodeData::IndexedAccess {
                object: named("Bag"),
                index: IndexKey::String("value".into()),
                origin: IndexedAccessOrigin::Written,
            });
            (check, string, one, two, false)
        }
        "keyof" => {
            let check = graph.intern_node(SemanticNodeData::KeyOf { base: named("Bag") });
            (check, string, one, two, true)
        }
        "template" => {
            let check =
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("5".into())));
            (check, template(), one, two, false)
        }
        "nested" => {
            let check = graph.intern_node(SemanticNodeData::Conditional {
                check: string,
                extends: number,
                true_branch_ref: string,
                false_branch_ref: number,
                distributive: false,
                pending: None,
            });
            (check, number, one, two, false)
        }
        "written" => (number, union(&[number, named("Missing")]), one, two, false),
        "infer" => (
            tuple(&[one, two]),
            tuple(&[infer, infer]),
            reference,
            two,
            false,
        ),
        "both" => (any, tuple(&[infer]), tuple(&[reference]), two, false),
        "distribution" => (union(&[one, two]), number, one, two, true),
        "literal_distribution" => (
            union(&[one, two]),
            union(&[one, two, literal(3.0)]),
            one,
            two,
            true,
        ),
        _ => panic!("unknown conditional fixture: {case}"),
    };
    (
        SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending: None,
        },
        case == "distribution",
    )
}

fn prepared_host() -> crate::VerterHost {
    let host = host();
    super::carrier_head_resolution_tests::upsert_ts(
        &host,FILE,"type A = number; type Bag = { value: string; other: number }; type Open<T> = T extends number ? 1 : 2; export {};",
    );
    host.ensure_indexed_ready(FILE)
        .expect("the fixture declarations are indexed");
    host
}

fn counters() -> [usize; 4] {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    [
        Dispatch::conditional_operand_walk_visits_for_tests(),
        Dispatch::conditional_operand_preparations_for_tests(),
        Dispatch::conditional_distributed_members_for_tests(),
        Dispatch::conditional_branch_selections_for_tests(),
    ]
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    text: String,
    keys: Vec<SemanticQueryKey>,
    work: [usize; 4],
    resumes: u64,
    partial: bool,
    cold_partial: bool,
    suppressed: bool,
    diagnostics: Vec<String>,
    descents: [usize; 2],
}

fn observe(case: &str, stepped: bool) -> Observation {
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, lowered) = fixture(&host, &dispatch, case);
    let SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch,
        false_branch,
        distributive,
        ..
    } = &key
    else {
        unreachable!()
    };
    let before = counters();
    let resumed_before = dispatch
        .graph_arc()
        .stats_snapshot()
        .continuation_stages_resumed;
    let completeness = ColdComputeCompletenessScope::enter();
    let mut descents = [0; 2];
    let (output, requested) = if stepped {
        dispatch.with_conditional_read_for_tests(&key, lowered, false, |read| {
            let mut requested = Vec::new();
            let mut lowered = [None; 2];
            let mut step = dispatch.advance_conditional_read_for_tests(read, None, None);
            loop {
                step = match step {
                    ConditionalProgressForTests::Done(output) => break (*output, requested),
                    ConditionalProgressForTests::Read(key) => {
                        requested.push(key.clone());
                        assert!(
                            requested.len() <= 64,
                            "a finite decision must not replay its reads: {case}"
                        );
                        let mut key = Some(key);
                        dispatch.advance_conditional_read_for_tests(
                            read,
                            Some(&mut || {
                                let answer = dispatch.execute_read(key.take().expect("one read"));
                                if case == "tuple" {
                                    dispatch.taint_conditional_probe_for_tests();
                                }
                                answer
                            }),
                            None,
                        )
                    }
                    ConditionalProgressForTests::Branch(branch) => {
                        let index = usize::from(branch);
                        let node = *lowered[index].get_or_insert_with(|| {
                            descents[index] += 1;
                            if branch {
                                *true_branch
                            } else {
                                *false_branch
                            }
                        });
                        dispatch.advance_conditional_read_for_tests(read, None, Some(node))
                    }
                };
            }
        })
    } else if lowered {
        dispatch.trace_conditional_from_lowering_for_tests(
            *check,
            *extends,
            *distributive,
            &mut |branch| {
                descents[usize::from(branch)] += 1;
                if branch {
                    *true_branch
                } else {
                    *false_branch
                }
            },
        )
    } else {
        dispatch.trace_conditional_family_for_tests(&key)
    };
    let cold_partial = current_cold_compute_completeness().is_partial();
    completeness.discard();
    let after = counters();
    let text = match &output.result {
        QueryResult::Value(node) => {
            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                &dispatch, *node, 0,
            )
        }
        other => format!("{other:?}"),
    };
    Observation {
        text,
        keys: requested,
        work: std::array::from_fn(|i| after[i] - before[i]),
        resumes: dispatch
            .graph_arc()
            .stats_snapshot()
            .continuation_stages_resumed
            - resumed_before,
        partial: output.result_is_partial(),
        cold_partial,
        suppressed: output.cache_suppress,
        diagnostics: output
            .walker_diagnostics
            .iter()
            .map(|diagnostic| format!("{diagnostic:?}"))
            .collect(),
        descents,
    }
}

#[test]
fn stepped_conditional_reads_preserve_in_place_answers_order_and_work() {
    for case in CASES {
        let stepped = observe(case, true);
        let in_place = observe(case, false);
        assert_eq!(stepped, in_place, "{case}");
        assert!(
            stepped.work[1] > 0,
            "the decision prepares its operands: {case}"
        );
    }
    assert!(observe("nested", true)
        .keys
        .iter()
        .any(|key| key.tag() == SemanticQueryKeyTag::Conditional));
    assert!(observe("tuple", true)
        .keys
        .iter()
        .any(|key| key.tag() == SemanticQueryKeyTag::TemplateLiteralReduce));
    assert!(observe("template", true)
        .keys
        .iter()
        .any(|key| key.tag() == SemanticQueryKeyTag::TemplateLiteralReduce));
    assert!(observe("indexed", true)
        .keys
        .iter()
        .any(|key| key.tag() == SemanticQueryKeyTag::IndexedAccess));
}

#[test]
fn lowered_branches_are_memoized_across_distributed_members_and_both_selection() {
    let distributed = observe("distribution", false);
    assert_eq!(distributed.descents, [0, 1]);
    assert_eq!(distributed.work[2], 2);
    assert_eq!(distributed.text, "1");
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, _) = fixture(&host, &dispatch, "both");
    let SemanticQueryKey::Conditional {
        check,
        extends,
        true_branch,
        false_branch,
        ..
    } = key
    else {
        unreachable!()
    };
    let mut descents = [0; 2];
    let output =
        dispatch.conditional_build_from_lowering_for_tests(check, extends, false, &mut |branch| {
            descents[usize::from(branch)] += 1;
            if branch {
                true_branch
            } else {
                false_branch
            }
        });
    assert_eq!(descents, [1, 1]);
    assert!(!output.result_is_partial());
}

#[test]
fn abandoning_each_standing_conditional_operation_releases_accounting_and_observations() {
    let mut standing = std::collections::BTreeSet::new();
    for (case, framed) in CASES.iter().flat_map(|&case| [(case, false), (case, true)]) {
        for stop_at in 0..16 {
            let host = prepared_host();
            let dispatch = ProjectSemanticDispatch::new(&host);
            let _construction = dispatch.enter_construction_scope();
            let (key, _) = fixture(&host, &dispatch, case);
            let SemanticQueryKey::Conditional {
                true_branch,
                false_branch,
                ..
            } = key
            else {
                unreachable!()
            };
            let before = dispatch.open_request_state_for_tests();
            let observations = dispatch.conditional_observation_depth_for_tests();
            let mut done = false;
            dispatch.with_conditional_read_for_tests(&key, true, framed, |read| {
                let mut step = dispatch.advance_conditional_read_for_tests(read, None, None);
                for at in 0..=stop_at {
                    if matches!(step, ConditionalProgressForTests::Done(_)) {
                        done = true;
                        break;
                    }
                    standing.insert(dispatch.conditional_standing_operation_for_tests(read));
                    if at == stop_at {
                        dispatch.abandon_conditional_read_for_tests(read);
                        break;
                    }
                    step = match step {
                        ConditionalProgressForTests::Read(key) => {
                            let mut key = Some(key);
                            dispatch.advance_conditional_read_for_tests(
                                read,
                                Some(&mut || {
                                    dispatch.execute_read(key.take().expect("one delivery"))
                                }),
                                None,
                            )
                        }
                        ConditionalProgressForTests::Branch(branch) => dispatch
                            .advance_conditional_read_for_tests(
                                read,
                                None,
                                Some(if branch { true_branch } else { false_branch }),
                            ),
                        ConditionalProgressForTests::Done(_) => unreachable!(),
                    };
                }
            });
            assert_eq!(
                dispatch.open_request_state_for_tests(),
                before,
                "{case} at {stop_at}"
            );
            assert_eq!(
                dispatch.conditional_observation_depth_for_tests(),
                observations,
                "{case} at {stop_at}"
            );
            if done {
                break;
            }
            assert!(stop_at < 15, "the bounded fixture must complete: {case}");
        }
    }
    for operation in [
        "branch",
        "conditional operand",
        "template",
        "evaluation",
        "structural fact",
        "key set",
        "genericness",
        "distribution",
        "operand walk",
    ] {
        assert!(
            standing.contains(operation),
            "an abandoned {operation} is covered: {standing:?}"
        );
    }
}

#[test]
fn a_conditional_operand_read_yields_without_repeating_its_preparation() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let inner_key = SemanticQueryKey::Conditional {
        check: string,
        extends: number,
        true_branch: string,
        false_branch: number,
        distributive: false,
        pending: None,
    };
    let inner = graph.intern_node(SemanticNodeData::Conditional {
        check: string,
        extends: number,
        true_branch_ref: string,
        false_branch_ref: number,
        distributive: false,
        pending: None,
    });
    let outer = SemanticQueryKey::Conditional {
        check: inner,
        extends: number,
        true_branch: string,
        false_branch: number,
        distributive: false,
        pending: None,
    };
    let (producer, lease) = hold_producer(&host, graph, inner_key);
    let waker = Waker::noop().clone();
    dispatch
        .with_semantic_task_for_tests(outer, |slot| {
            let task = slot.as_mut().expect("the conditional owns its task");
            let preparations = || {
                ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::conditional_operand_preparations_for_tests()
            };
            let before = preparations();
            {
                let _waits = verter_execution::blocking::WaitProbe::forbid();
                assert!(task.poll_read_for_tests(&waker).is_pending());
            }
            assert_eq!(preparations() - before, 1);
            let held = preparations();
            {
                let _waits = verter_execution::blocking::WaitProbe::forbid();
                assert!(task.poll_read_for_tests(&waker).is_pending());
            }
            assert_eq!(preparations(), held, "a held read executes no preparation");
            drop(lease);
            let Poll::Ready(Ok(answer)) = task.poll_read_for_tests(&waker) else {
                panic!("the released operand must complete");
            };
            assert!(matches!(
                answer.value(),
                QueryResult::Value(SemanticQueryValue::TypeNode(node)) if *node == string
            ));
            assert!(answer.fact_status().is_complete());
            assert!(answer.walker_diagnostics().is_empty());
            assert!(answer.receipt().priced().is_some());
            // The inner conditional prepares once after release. The
            // outer one continues at the conditional-operand delivery.
            assert_eq!(preparations() - held, 1);
        })
        .expect("the conditional enters the continuation runtime");
    drop(producer);
}

/// A decision whose declared check names an alias reads the alias's
/// declaration as the decision's own request: while another task holds
/// that `ResolveDecl`, the polled decision waits without blocking and
/// without preparing or selecting again, and once it is released the
/// decision continues where it stood and answers as the synchronous entry
/// does.
#[test]
fn a_held_declaration_read_suspends_the_decision_without_repeating_its_work() {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, _) = fixture(&host, &dispatch, "declared");
    let SemanticQueryKey::Conditional { true_branch, .. } = &key else {
        unreachable!()
    };
    let true_branch = *true_branch;
    // The decision's first read, named without being made.
    let declaration = dispatch.with_conditional_read_for_tests(&key, false, true, |read| {
        let ConditionalProgressForTests::Read(first) =
            dispatch.advance_conditional_read_for_tests(read, None, None)
        else {
            panic!("the alias check is read before the decision completes");
        };
        dispatch.abandon_conditional_read_for_tests(read);
        first
    });
    assert_eq!(declaration.tag(), SemanticQueryKeyTag::ResolveDecl);
    let graph = host.project_type_store().semantic_graph();
    let (producer, lease) = hold_producer(&host, graph, declaration);
    let waker = Waker::noop().clone();
    let work = || {
        [
            Dispatch::conditional_operand_preparations_for_tests(),
            Dispatch::conditional_branch_selections_for_tests(),
        ]
    };
    dispatch
        .with_semantic_task_for_tests(key.clone(), |slot| {
            let task = slot.as_mut().expect("the conditional owns its task");
            let before = work();
            {
                let _waits = verter_execution::blocking::WaitProbe::forbid();
                assert!(task.poll_read_for_tests(&waker).is_pending());
            }
            let held = work();
            assert_eq!(
                [held[0] - before[0], held[1] - before[1]],
                [1, 0],
                "the decision prepares once and stops at the declaration"
            );
            {
                let _waits = verter_execution::blocking::WaitProbe::forbid();
                assert!(task.poll_read_for_tests(&waker).is_pending());
            }
            assert_eq!(work(), held, "a held read prepares and selects nothing");
            drop(lease);
            let Poll::Ready(Ok(answer)) = task.poll_read_for_tests(&waker) else {
                panic!("the released declaration must complete the decision");
            };
            let after = work();
            assert_eq!(
                [after[0] - held[0], after[1] - held[1]],
                [0, 1],
                "the delivery continues the preparation it held and selects once"
            );
            assert!(matches!(
                answer.value(),
                QueryResult::Value(SemanticQueryValue::TypeNode(node)) if *node == true_branch
            ));
            assert!(answer.fact_status().is_complete());
            assert!(answer.walker_diagnostics().is_empty());
            assert!(answer.receipt().priced().is_some());
        })
        .expect("the conditional enters the continuation runtime");
    drop(producer);
    let synchronous = dispatch.trace_conditional_family_for_tests(&key).0;
    assert!(matches!(synchronous.result, QueryResult::Value(node) if node == true_branch));
    assert!(!synchronous.result_is_partial());
    assert!(synchronous.walker_diagnostics.is_empty());
}

#[test]
fn literal_distribution_requests_no_conditional_member_reads() {
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, _) = fixture(&host, &dispatch, "literal_distribution");
    let SemanticQueryKey::Conditional { true_branch, .. } = &key else {
        unreachable!()
    };
    let (output, requested) = dispatch.trace_conditional_family_for_tests(&key);
    assert!(matches!(&output.result, QueryResult::Value(node) if node == true_branch));
    assert!(!output.result_is_partial());
    assert!(output.walker_diagnostics.is_empty());
    assert!(
        requested
            .iter()
            .all(|key| !matches!(key, SemanticQueryKey::Conditional { .. })),
        "literal distribution must charge members in place: {requested:?}",
    );
}

/// A literal distribution decides its members in place, each charged and
/// refused as that member's own conditional query would be. When the request
/// budget expires on the second member, its refusal is that member's answer:
/// the distribution folds both members and still reads its union
/// normalization, whose refusal is the conditional's partial answer. The
/// refusal never replaces the whole distribution, which would fold one member
/// and skip the normalization.
#[test]
fn a_refused_in_place_member_is_folded_and_normalized_like_a_member_query() {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    // The answer, its partiality, the members folded and whether the
    // distribution read its union normalization, under `budget`.
    let observe = |budget: usize| {
        let host = prepared_host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let _construction = dispatch.enter_construction_scope();
        let (key, _) = fixture(&host, &dispatch, "literal_distribution");
        let _request = verter_type_engine::request_context::RequestContextGuard::install(
            verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
                1,
                Arc::from(FILE),
                verter_audit::RequestKind::ComponentMeta,
                false,
                false,
                None,
                budget,
            ),
        );
        let members = Dispatch::conditional_distributed_members_for_tests();
        let (output, requested) = dispatch.trace_conditional_family_for_tests(&key);
        let text = match &output.result {
            QueryResult::Value(node) => {
                crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                    &dispatch, *node, 0,
                )
            }
            other => format!("{other:?}"),
        };
        (
            text,
            output.result_is_partial(),
            Dispatch::conditional_distributed_members_for_tests() - members,
            requested
                .iter()
                .any(|key| key.tag() == SemanticQueryKeyTag::ReduceUnion),
        )
    };
    assert_eq!(
        observe(1_000_000),
        ("1".to_owned(), false, 2, true),
        "an unbounded request"
    );
    // One operation answers the first member; the second member's spend
    // exceeds it.
    assert_eq!(
        observe(1),
        (
            "Error(IncompleteSemanticOperand { reasons: PartialReasonSet(2048) })".to_owned(),
            true,
            2,
            true
        ),
        "the refused second member is folded and the union is normalized"
    );
}

/// A synchronous decision of `case` outside every running execution,
/// decided by the synchronous family driver or by the in-place decision
/// driver: its answer and partiality, and the dispatch intents it records,
/// as the conditional decisions and hops a captured request's
/// type-resolution audit counts for them.
fn synchronous_conditional_intents(case: &str, family: bool) -> (String, bool, [u64; 2]) {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, _) = fixture(&host, &dispatch, case);
    let before = Dispatch::dispatch_intents_for_tests();
    let output = if family {
        dispatch.trace_conditional_family_for_tests(&key).0
    } else {
        dispatch.decide_materialized_conditional_in_place_for_tests(&key)
    };
    let after = Dispatch::dispatch_intents_for_tests();
    let text = match &output.result {
        QueryResult::Value(node) => {
            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                &dispatch, *node, 0,
            )
        }
        other => format!("{other:?}"),
    };
    (
        text,
        output.result_is_partial(),
        [after[0] - before[0], after[1] - before[1]],
    )
}

/// The synchronous family driver counts the dispatch intent of every read
/// the decision makes where it reads it, as the in-place decision driver
/// does, even when the read itself is driven on the continuation runtime:
/// the driven root enters its query without counting it. The nested
/// conditional operand is one conditional decision and a hop.
#[test]
fn a_synchronous_conditional_family_counts_its_nested_conditional_intent() {
    let native = synchronous_conditional_intents("nested", false);
    assert_eq!(native.0, "1", "the nested conditional decides: {native:?}");
    assert_eq!(
        native.2[0], 1,
        "the nested operand is one conditional decision: {native:?}"
    );
    assert!(native.2[1] >= 1, "the nested operand is a hop: {native:?}");
    assert_eq!(synchronous_conditional_intents("nested", true), native);
}

/// Every read a materialized decision makes counts the same dispatch intent
/// under the synchronous family driver as under the in-place decision
/// driver, whether the read is a declaration, an instantiation, a carrier
/// evaluation or a nested conditional, and whether or not it is driven on
/// the continuation runtime.
#[test]
fn every_synchronous_conditional_counts_the_intents_its_in_place_driver_counts() {
    for case in CASES {
        let native = synchronous_conditional_intents(case, false);
        assert!(
            !native.0.contains("ExecutionAborted"),
            "{case}: the decision completes: {native:?}"
        );
        assert_eq!(
            synchronous_conditional_intents(case, true),
            native,
            "{case}"
        );
    }
}

/// A closed `keyof` check distributes over the literal keys its base
/// denotes. The decision reads that key set through the key-set walk its own
/// operation holds: the `keyof` read is a read the decision names and stands
/// at, never one answered in place, and the stepped decision answers, reads
/// and works exactly as the synchronous one.
#[test]
fn a_keyof_check_distributes_over_a_key_set_the_decision_reads() {
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    let before = Dispatch::conditional_in_place_answers_for_tests();
    let stepped = observe("keyof", true);
    let in_place = observe("keyof", false);
    let native = synchronous_conditional_intents("keyof", false);
    assert_eq!(
        Dispatch::conditional_in_place_answers_for_tests(),
        before,
        "no read of the keyof check is answered in place"
    );
    assert_eq!(stepped, in_place);
    assert_eq!(stepped.text, "1");
    assert_eq!(native.0, "1");
    assert_eq!(stepped.work[2], 2, "each key is a distributed member");
    assert!(
        stepped
            .keys
            .iter()
            .any(|key| key.tag() == SemanticQueryKeyTag::KeyOf),
        "the decision names its keyof read: {:?}",
        stepped.keys
    );
    let host = prepared_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let (key, _) = fixture(&host, &dispatch, "keyof");
    dispatch.with_conditional_read_for_tests(&key, false, false, |read| {
        let mut step = dispatch.advance_conditional_read_for_tests(read, None, None);
        for _ in 0..16 {
            let ConditionalProgressForTests::Read(key) = step else {
                panic!("the decision reads its key set before it completes");
            };
            if key.tag() == SemanticQueryKeyTag::KeyOf {
                assert_eq!(
                    dispatch.conditional_standing_operation_for_tests(read),
                    "key set"
                );
                return;
            }
            let mut key = Some(key);
            step = dispatch.advance_conditional_read_for_tests(
                read,
                Some(&mut || dispatch.execute_read(key.take().expect("one delivery"))),
                None,
            );
        }
        panic!("the decision must reach its keyof read");
    });
}

const KEY_SET_FILE: &str = "/key-set.ts";
const KEY_SET_SOURCE: &str = "export type Bag = { value: string; other: number };\n\
                              function h() { return class { y = 2; z = 3 }; }\n\
                              export type IH = InstanceType<ReturnType<typeof h>>;\n\
                              export type Open<T> = T;\n";

/// The key-set walk's subject for `case`.
fn key_set_subject(
    host: &crate::VerterHost,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    case: &str,
) -> SemanticNodeId {
    let graph = dispatch.graph_arc();
    let identity = |name: &str| DeclIdentity {
        canonical_id: KEY_SET_FILE.into(),
        owner: TopLevelOwnerId::ordinary_file(),
        whole_hash: host
            .shallow_file_state(KEY_SET_FILE)
            .expect("indexed declarations")
            .whole_hash,
        decl_name: name.into(),
    };
    let named = |name| {
        graph.intern_node(SemanticNodeData::DeclRef {
            identity: identity(name),
        })
    };
    match case {
        "declaration" | "partial" => named("Bag"),
        "class" => named("IH"),
        "open" => graph.intern_node(SemanticNodeData::TypeParam {
            decl: identity("Open"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: "T".into(),
        }),
        // The surface `Bag` settles to: the base of the declaration walk's
        // second read.
        "object" => match dispatch
            .trace_key_set_read_for_tests(named("Bag"), true, false)
            .requested
            .get(1)
        {
            Some(SemanticQueryKey::KeyOf { base, .. }) => *base,
            other => panic!("the declaration walk reads its surface's keys: {other:?}"),
        },
        _ => panic!("unknown key-set fixture: {case}"),
    }
}

/// The key-set walk stepped by hand answers, reads and folds exactly what
/// `key_set_of`'s in-place driver does, over the same steps: a plain
/// object's keys in one read; a declaration whose `keyof` keeps its carrier,
/// settled to its surface and read again; an alias of a class
/// expression's instance, settled to the instance, whose `keyof` keeps the
/// class as its origin and settles through the surface the instance reads
/// through; a carrier that settles to
/// itself, which keeps its keys open; and a read that stopped short, whose
/// partiality folds into the caller.
#[test]
fn stepped_key_set_reads_preserve_in_place_answers_order_and_completeness() {
    for (case, reads, keys) in [
        ("object", 1, Some(&["\"other\"", "\"value\""][..])),
        ("declaration", 2, Some(&["\"other\"", "\"value\""][..])),
        ("class", 3, Some(&["\"y\"", "\"z\""][..])),
        ("open", 1, None),
        ("partial", 1, None),
    ] {
        let host = host();
        super::carrier_head_resolution_tests::upsert_ts(&host, KEY_SET_FILE, KEY_SET_SOURCE);
        host.ensure_indexed_ready(KEY_SET_FILE)
            .expect("the fixture declarations are indexed");
        let fixture = ProjectSemanticDispatch::new(&host);
        let _construction = fixture.enter_construction_scope();
        let base = key_set_subject(&host, &fixture, case);
        let partial = case == "partial";
        fixture.graph().invalidate_all();
        let stepped =
            ProjectSemanticDispatch::new(&host).trace_key_set_read_for_tests(base, true, partial);
        // The same operands with an independently cold memo.
        fixture.graph().invalidate_all();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let in_place = dispatch.trace_key_set_read_for_tests(base, false, partial);
        assert_eq!(stepped, in_place, "{case}");
        assert_eq!(stepped.requested.len(), reads, "{case}: {stepped:?}");
        assert!(
            stepped
                .requested
                .iter()
                .all(|key| key.tag() == SemanticQueryKeyTag::KeyOf),
            "{case}: the walk names only keyof reads"
        );
        let kept = |at: usize| match stepped.answers[at]
            .and_then(|node| dispatch.graph().node_data(node))
            .as_deref()
        {
            Some(SemanticNodeData::KeyOf { base: kept }) => Some(*kept),
            _ => None,
        };
        match case {
            "declaration" | "open" => assert_eq!(
                kept(0),
                Some(base),
                "{case}: the read keeps its keyof carrier"
            ),
            "class" => assert!(
                kept(0) == Some(base)
                    && kept(1).is_some_and(|kept| matches!(
                        dispatch.graph().node_data(kept).as_deref(),
                        Some(SemanticNodeData::ClassExpressionInstance { .. })
                    )),
                "the alias keeps its keyof carrier, then the instance keeps its class expression's"
            ),
            _ => {}
        }
        if reads > 1 {
            assert!(
                matches!(
                    &stepped.requested[reads - 1],
                    SemanticQueryKey::KeyOf { base: settled, .. }
                        if matches!(
                            dispatch.graph().node_data(*settled).as_deref(),
                            Some(SemanticNodeData::Object(_))
                        )
                ),
                "{case}: the last read is of the settled surface"
            );
        }
        match keys {
            Some(expected) => {
                let node = stepped.keys.expect("the keys settle");
                let text = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                    &dispatch, node, 0,
                );
                let members = text
                    .strip_prefix("Union(")
                    .and_then(|members| members.strip_suffix(')'))
                    .unwrap_or_else(|| panic!("{case}: the keys are a union: {text}"));
                let mut members: Vec<_> = members.split(" | ").collect();
                members.sort_unstable();
                assert_eq!(members, expected, "{case}");
            }
            None => assert_eq!(stepped.keys, None, "{case}"),
        }
        assert_eq!(stepped.observations_complete, !partial, "{case}");
        assert_eq!(
            stepped.cache_suppress, partial,
            "{case}: a folded partial read suppresses the cache"
        );
        assert_eq!(
            stepped
                .partial_reasons
                .contains(verter_type_engine::semantic_query::PartialReasonSet::MISSING_DEPENDENCY),
            partial,
            "{case}: {:?}",
            stepped.partial_reasons
        );
    }
}
