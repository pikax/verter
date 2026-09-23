//! Signature-kernel epoch replacement, seen from the `SignaturesOfType` and
//! `ReadSignatureResult` consumers.
//!
//! A replacement retires every kernel handle a warm memo value carries. The
//! contract these tests pin: a value of a retired epoch is a MISS that
//! recomputes the same answer in the current epoch — on the warm path, for
//! a consumer whose pin lands after the replacement, for the caller and the
//! joiners of a build that straddles it, and across a long run of unique
//! edits — and never an incomplete answer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};

use super::super::call_resolve_tests::{occurrence, signature};
use super::super::ProjectSemanticDispatch;
use super::{SharedSignatureNodes, SignaturesOfTypeBuildPoint, SIGNATURES_OF_TYPE_BUILD_HOOK};
use crate::semantic_query::{
    FunctionParam, NodeScopeId, PrimitiveKind, QueryResult, SemanticContextId, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryValue, SignatureKind,
    SignatureReturnCarrier, CONTEXT_FREE_EVALUATION,
};
use crate::signature_kernel::{
    BorrowedSet, CallSubstitution, GraphEpoch, ReadSignatureResultKey, ResultDemand,
    SemanticReadView, SignatureCandidate, SignatureSetValue,
};
use crate::types::UpsertRequest;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;
use crate::{HostConfig, VerterHost};

const CANONICAL: &str = "/ws/signature-epoch.ts";

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from("export {};\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn prim(d: &ProjectSemanticDispatch<'_>, kind: PrimitiveKind) -> SemanticNodeId {
    d.graph().intern_node(SemanticNodeData::Primitive(kind))
}

fn param(ty: SemanticNodeId) -> FunctionParam {
    FunctionParam::synthetic(None, ty, false, false)
}

fn callable(d: &ProjectSemanticDispatch<'_>, calls: Vec<SemanticNodeId>) -> SemanticNodeId {
    d.graph()
        .intern_node(SemanticNodeData::Object(crate::test_surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(calls.into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

/// Two signature-bearing subjects: an overloaded callable (a set of two
/// authored leaves) and a union of two callables whose parameters agree (one
/// composite candidate whose node form is built through
/// `ReadSignatureResult`).
fn subjects(d: &ProjectSemanticDispatch<'_>) -> [SemanticNodeId; 2] {
    let string = prim(d, PrimitiveKind::String);
    let number = prim(d, PrimitiveKind::Number);
    let sig = |name, ordinal, param_ty, return_ty| {
        signature(
            d,
            name,
            ordinal,
            SignatureKind::Call,
            vec![param(param_ty)],
            vec![],
            return_ty,
        )
    };
    let overloaded = callable(
        d,
        vec![
            sig("over", 0, string, number),
            sig("over", 1, number, string),
        ],
    );
    let union = d.intern_normalized_union_or_intersection(
        &[
            sig("left", 0, string, string),
            sig("right", 0, string, number),
        ],
        true,
    );
    [overloaded, union]
}

/// The shared nodes of `subject`, rendered structurally so two hosts'
/// answers compare; an incomplete read renders as its reason.
fn rendered(d: &ProjectSemanticDispatch<'_>, read: SharedSignatureNodes) -> Vec<String> {
    match read {
        SharedSignatureNodes::Nodes(nodes) => {
            nodes.iter().map(|node| render_node(d, *node, 0)).collect()
        }
        SharedSignatureNodes::Incomplete(reason) => vec![format!("incomplete: {reason:?}")],
    }
}

fn shared(d: &ProjectSemanticDispatch<'_>, subject: SemanticNodeId) -> Vec<String> {
    rendered(d, d.shared_signature_nodes(subject, SignatureKind::Call))
}

/// The same subjects' answers on a fresh host — every one complete.
fn fresh_answers() -> Vec<Vec<String>> {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let answers: Vec<Vec<String>> = subjects(&d)
        .iter()
        .map(|subject| shared(&d, *subject))
        .collect();
    for answer in &answers {
        assert!(
            !answer.is_empty() && !answer[0].starts_with("incomplete"),
            "premise: a fresh host answers every subject: {answers:?}"
        );
    }
    answers
}

/// Run `body` with `during` hooked at `point` of every `SignaturesOfType`
/// build on this thread.
fn with_build_hook<R>(
    point: SignaturesOfTypeBuildPoint,
    during: impl Fn() + 'static,
    body: impl FnOnce() -> R,
) -> R {
    SIGNATURES_OF_TYPE_BUILD_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move |at| {
            if at == point {
                during();
            }
        }));
    });
    let out = body();
    SIGNATURES_OF_TYPE_BUILD_HOOK.with(|hook| *hook.borrow_mut() = None);
    out
}

fn set_key(subject: SemanticNodeId) -> SemanticQueryKey {
    SemanticQueryKey::SignaturesOfType {
        subject,
        kind: SignatureKind::Call,
        context: SemanticContextId::production(),
    }
}

fn set_epoch(value: &SignatureSetValue) -> Option<GraphEpoch> {
    value.set.epoch()
}

fn candidates(
    d: &ProjectSemanticDispatch<'_>,
    value: &SignatureSetValue,
) -> Vec<SignatureCandidate> {
    match SemanticReadView::pin(d.graph().signature_store())
        .read_set(value.set)
        .expect("a current-epoch set reads")
    {
        BorrowedSet::Empty => Vec::new(),
        BorrowedSet::One { candidate, .. } => vec![candidate],
        BorrowedSet::Many(list) => list.to_vec(),
    }
}

/// A warm `SignaturesOfType` value whose epoch was replaced is a miss: the
/// next read recomputes in the current epoch, replaces the retired candidate
/// in its slot, and answers exactly what it answered before — for authored
/// leaves and for a composite alike.
///
/// Without the memo's kernel-epoch gate the warm read serves the retired set
/// again; its pin meets `StaleHandle`, the one re-dispatch is served the same
/// retired set, and both reads come back `Incomplete(UnsettledInput)`.
#[test]
fn a_warm_signature_set_of_a_retired_epoch_is_a_miss_that_recomputes_the_answer() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let store = d.graph().signature_store();
    for subject in subjects(&d) {
        let before = shared(&d, subject);
        assert!(!before[0].starts_with("incomplete"), "premise: {before:?}");
        let warm = d.signature_set_value(subject, SignatureKind::Call).unwrap();
        // A bare replacement: no hygiene sweep, so the retired value stays in
        // the memo and only the warm gate stands between it and a reader.
        let current = store.replace_epoch().unwrap();
        assert!(
            set_epoch(&warm).is_some_and(|epoch| epoch != current),
            "premise: the warm set's handles are retired"
        );
        assert_eq!(shared(&d, subject), before);
        let reread = d.signature_set_value(subject, SignatureKind::Call).unwrap();
        assert_eq!(
            set_epoch(&reread),
            Some(current),
            "the retired value was served warm instead of recomputed"
        );
        assert_eq!(
            d.graph().slot_candidate_count_for_tests(&set_key(subject)),
            1,
            "the recompute replaces the retired candidate in its slot"
        );
    }
}

/// The cap-driven compaction evicts every `SignaturesOfType` candidate and
/// every `ReadSignatureResult` family of the retired epoch, keeps the memo's
/// retention ledger in step with it, and leaves the answers a fresh host
/// gives. A `ReadSignatureResult` of retired handles is never served again:
/// the consumer re-derives its key in the current epoch, and that key is a
/// miss that forces the same result.
#[test]
fn compaction_evicts_retired_kernel_families_and_the_answers_recompute() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let graph = d.graph();
    let store = graph.signature_store();
    let [overloaded, union] = subjects(&d);
    let before = [shared(&d, overloaded), shared(&d, union)];

    let result_key = |value: &SignatureSetValue| {
        let candidate = candidates(&d, value)[0];
        let view = SemanticReadView::pin(store);
        let space = view
            .descriptor(candidate.signature)
            .unwrap()
            .residual_binders;
        let call = store
            .intern_substitution(CallSubstitution::identity(space), None)
            .unwrap();
        SemanticQueryKey::ReadSignatureResult(ReadSignatureResultKey {
            descriptor: candidate.signature,
            call_substitution: call,
            projection: ResultDemand::Return,
            evaluation: CONTEXT_FREE_EVALUATION,
            semantic_context: SemanticContextId::production(),
        })
    };
    let return_node = |key: &SemanticQueryKey| match d.execute(key.clone()) {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::SignatureResult(value) => {
                let record = store.applied_result(value.result).unwrap();
                store.type_token_node(record.return_type.unwrap()).unwrap()
            }
            other => panic!("expected a signature result, got {other:?}"),
        },
        other => panic!("expected a value, got {other:?}"),
    };
    let set = d
        .signature_set_value(overloaded, SignatureKind::Call)
        .unwrap();
    let retired_key = result_key(&set);
    let answered = return_node(&retired_key);
    assert_eq!(graph.slot_candidate_count_for_tests(&retired_key), 1);
    let families = graph.memo_family_count_for_test();

    let current = graph
        .compact_signature_store_over(0)
        .expect("a non-empty store is over a zero cap");
    assert_eq!(store.interned_len(), 0);
    for key in [set_key(overloaded), set_key(union), retired_key.clone()] {
        assert_eq!(
            graph.slot_candidate_count_for_tests(&key),
            0,
            "{key:?} names the retired epoch and must be evicted"
        );
    }
    assert!(graph.memo_family_count_for_test() <= families - 3);
    assert_eq!(
        graph.memo_family_count_for_test(),
        graph.memo_budget_tracked_len_for_test(),
        "every evicted family leaves the retention ledger with it"
    );

    assert_eq!([shared(&d, overloaded), shared(&d, union)], before);
    assert_eq!(fresh_answers(), before.to_vec());
    let set = d
        .signature_set_value(overloaded, SignatureKind::Call)
        .unwrap();
    assert_eq!(set_epoch(&set), Some(current));
    let key = result_key(&set);
    assert_ne!(key, retired_key, "the current epoch mints a new key");
    assert_eq!(return_node(&key), answered);
}

/// A `ReadSignatureResult` value of a retired epoch is never served warm,
/// even without the compaction sweep: its key's handles are retired, so the
/// read recomputes and reports the retired key as unanswerable instead of
/// handing out a result id no pin can read.
#[test]
fn a_warm_signature_result_of_a_retired_epoch_is_never_served() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let store = d.graph().signature_store();
    let [overloaded, _] = subjects(&d);
    let set = d
        .signature_set_value(overloaded, SignatureKind::Call)
        .unwrap();
    let candidate = candidates(&d, &set)[0];
    let space = SemanticReadView::pin(store)
        .descriptor(candidate.signature)
        .unwrap()
        .residual_binders;
    let key = SemanticQueryKey::ReadSignatureResult(ReadSignatureResultKey {
        descriptor: candidate.signature,
        call_substitution: store
            .intern_substitution(CallSubstitution::identity(space), None)
            .unwrap(),
        projection: ResultDemand::Return,
        evaluation: CONTEXT_FREE_EVALUATION,
        semantic_context: SemanticContextId::production(),
    });
    let warm = match d.execute(key.clone()) {
        QueryResult::Value(output) => output.value,
        other => panic!("expected a value, got {other:?}"),
    };
    store.replace_epoch().unwrap();
    match d.execute(key) {
        QueryResult::Value(output) => panic!(
            "a retired-epoch result was served: {:?} (was {warm:?})",
            output.value
        ),
        QueryResult::Recursive(_) | QueryResult::Error(_) => {}
    }
}

/// The race a replacement opens for every consumer: the memo read hands out
/// a warm set, the replacement lands, and only then does the consumer pin
/// its view. The pinned read meets `StaleHandle`; the consumer re-dispatches
/// once, the memo refuses the retired value, and the answer equals a fresh
/// host's — never `Incomplete`. Both consumer shapes (the node form, which
/// also builds a composite through `ReadSignatureResult`, and the
/// positional read) take the same path.
///
/// Without the re-dispatch the pinned read's `StaleHandle` is the answer:
/// `Incomplete(UnsettledInput)`.
#[test]
fn a_replacement_between_the_memo_read_and_the_pin_is_a_miss_not_an_incomplete_answer() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let graph = d.graph();
    let expected = fresh_answers();
    for (subject, expected) in subjects(&d).into_iter().zip(expected) {
        let _cold = d.signature_set_value(subject, SignatureKind::Call).unwrap();
        let hits = graph.stats_snapshot().hits;
        let looked_up = d.signature_set_value(subject, SignatureKind::Call).unwrap();
        assert!(
            graph.stats_snapshot().hits > hits,
            "premise: the lookup is a warm hit"
        );
        let positional = |value: SignatureSetValue| {
            d.read_signature_set(subject, SignatureKind::Call, Ok(value), |value| {
                d.raw_positional_reads(value, 0)
            })
            .map(|reads| {
                reads
                    .iter()
                    .map(|raw| (raw.receiver, raw.argument))
                    .collect::<Vec<_>>()
            })
        };
        let positional_before = positional(looked_up.clone());
        assert!(
            positional_before.is_ok(),
            "premise: the positional read settles: {positional_before:?}"
        );

        graph.compact_signature_store_over(0).expect("replaced");
        let read =
            d.shared_signature_nodes_from(subject, SignatureKind::Call, Ok(looked_up.clone()));
        assert_eq!(rendered(&d, read), expected);

        graph.compact_signature_store_over(0).expect("replaced");
        assert_eq!(positional(looked_up), positional_before);
    }
}

/// A build that straddles a replacement — its walk interned in the old
/// epoch, its value reaches the memo after the swap — publishes a value of a
/// retired epoch. Its own caller gets that value (and re-reads through the
/// consumer), but no later read is served it: the next read is a miss that
/// recomputes in the current epoch and replaces it in its slot.
#[test]
fn a_build_that_straddles_a_replacement_cannot_poison_later_reads() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let graph = Arc::clone(d.graph());
    let [overloaded, _] = subjects(&d);
    let expected = fresh_answers().remove(0);

    let swap = Arc::clone(&graph);
    let straddled = with_build_hook(
        SignaturesOfTypeBuildPoint::Settled,
        move || {
            let _ = swap.compact_signature_store_over(0);
        },
        || d.signature_set_value(overloaded, SignatureKind::Call),
    )
    .unwrap();
    let current = graph.signature_store().epoch();
    assert!(
        set_epoch(&straddled).is_some_and(|epoch| epoch != current),
        "premise: the build answered in the epoch its own walk saw retired"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&set_key(overloaded)),
        1,
        "premise: the retired value reached the memo"
    );

    let reread = d
        .signature_set_value(overloaded, SignatureKind::Call)
        .unwrap();
    assert_eq!(set_epoch(&reread), Some(current));
    assert_eq!(
        graph.slot_candidate_count_for_tests(&set_key(overloaded)),
        1
    );
    assert_eq!(
        rendered(
            &d,
            d.shared_signature_nodes_from(overloaded, SignatureKind::Call, Ok(straddled))
        ),
        expected,
        "the straddling build's own caller re-reads once and answers"
    );
}

/// A replacement that lands while a build's walk holds what it interned —
/// after discovery, before the set is published — retires those records, so
/// that walk cannot answer. The build walks once more in the current epoch:
/// its caller gets the current answer, never `Incomplete`.
///
/// Without the second walk the build fails (the set it publishes names
/// retired handles) and its caller reads `Incomplete(UnsettledInput)`.
#[test]
fn a_replacement_during_the_walk_is_walked_again_not_an_incomplete_answer() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let graph = Arc::clone(d.graph());
    let expected = fresh_answers();
    for (subject, expected) in subjects(&d).into_iter().zip(expected) {
        let swapped = Arc::new(AtomicBool::new(false));
        let (swap, once) = (Arc::clone(&graph), Arc::clone(&swapped));
        let read = with_build_hook(
            SignaturesOfTypeBuildPoint::Discovered,
            move || {
                if !once.swap(true, Ordering::SeqCst) {
                    swap.compact_signature_store_over(0).expect("replaced");
                }
            },
            || d.signature_set_value(subject, SignatureKind::Call),
        );
        assert!(
            swapped.load(Ordering::SeqCst),
            "premise: the replacement landed mid-walk"
        );
        let value = read.expect("the second walk answers");
        assert_eq!(set_epoch(&value), Some(graph.signature_store().epoch()));
        assert_eq!(shared(&d, subject), expected);
    }
}

/// A joiner parked on a build that straddles a replacement must not take
/// the winner's retired-epoch value, even when the winner's carrier
/// validates for the joiner's view (the subject's signatures are rooted in a
/// live file, so without the replacement the joiner shares the winner's
/// value). It forks and recomputes in the current epoch.
#[test]
fn a_joiner_never_takes_a_retired_epoch_value_from_the_build_it_joined() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let graph = Arc::clone(d.graph());
    let shallow = d
        .ctx
        .shallow_file_state(CANONICAL)
        .expect("the fixture file is indexed");
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(CANONICAL),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: shallow.whole_hash,
        local_scope: None,
    };
    let string = prim(&d, PrimitiveKind::String);
    let rooted = |name: &str| {
        let sig = graph.intern_node_with_scope(
            SemanticNodeData::Signature {
                kind: SignatureKind::Call,
                params: Arc::from(vec![param(string)].into_boxed_slice()),
                return_type: string,
                type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                occurrence: Some(occurrence(name, 0)),
                return_carrier: SignatureReturnCarrier::Declared(string),
                signature_span: None,
                return_type_span: None,
                predicate: None,
            },
            scope.clone(),
        );
        callable(&d, vec![sig])
    };

    // Winner parks after its walk; the joiner parks on the winner's flight;
    // `swap` optionally replaces the epoch before the winner publishes.
    let race = |subject: SemanticNodeId, swap: bool| {
        let built = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let joined_before = graph.test_joiner_on_condvar_count();
        std::thread::scope(|scope| {
            let winner = {
                let (built, release) = (Arc::clone(&built), Arc::clone(&release));
                let host = Arc::clone(&host);
                scope.spawn(move || {
                    let d = ProjectSemanticDispatch::new(host.as_ref());
                    with_build_hook(
                        SignaturesOfTypeBuildPoint::Settled,
                        move || {
                            built.wait();
                            release.wait();
                        },
                        || d.signature_set_value(subject, SignatureKind::Call),
                    )
                    .unwrap()
                })
            };
            built.wait();
            let joiner = {
                let host = Arc::clone(&host);
                scope.spawn(move || {
                    let d = ProjectSemanticDispatch::new(host.as_ref());
                    d.signature_set_value(subject, SignatureKind::Call).unwrap()
                })
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while graph.test_joiner_on_condvar_count() == joined_before {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the joiner never parked"
                );
                std::thread::yield_now();
            }
            if swap {
                graph.compact_signature_store_over(0).expect("replaced");
            }
            release.wait();
            (winner.join().unwrap(), joiner.join().unwrap())
        })
    };

    let forks = graph.test_joiner_view_mismatch_forks();
    let (won, joined) = race(rooted("shared"), false);
    assert_eq!(
        joined, won,
        "premise: without a replacement the joiner shares the winner's value"
    );
    assert_eq!(
        graph.test_joiner_view_mismatch_forks(),
        forks,
        "premise: the winner's carrier validates for the joiner's view"
    );

    let (won, joined) = race(rooted("straddled"), true);
    let current = graph.signature_store().epoch();
    assert!(
        set_epoch(&won).is_some_and(|epoch| epoch != current),
        "premise: the winner answered in the retired epoch"
    );
    assert_eq!(
        set_epoch(&joined),
        Some(current),
        "the joiner took the winner's retired-epoch value"
    );
    assert_eq!(graph.test_joiner_view_mismatch_forks(), forks + 1);
}

const CHURN: &str = "/wb/signature_epoch_churn.ts";

/// Signature-heavy witnesses, each answered through the public audited
/// flow-return boundary.
const WITNESSES: [&str; 7] = [
    "witnessOverload",
    "witnessIntersection",
    "witnessGeneric",
    "witnessReturnType",
    "witnessParameters",
    "witnessIntersectionReturn",
    "witnessUnique",
];

/// One version of the churned module. Every version is UNIQUE content (its
/// `tagged{cycle}` signature and literal differ), so each edit re-interns
/// the module's signatures under fresh nodes.
///
/// Measured on TypeScript 7.0.2 (`tsc --declaration --emitDeclarationOnly
/// --strict`, version 7): `witnessOverload(): number`,
/// `witnessIntersection(): string`, `witnessGeneric(): [string, number]`,
/// `witnessReturnType(): string`, `witnessParameters(): [x: number]`,
/// `witnessIntersectionReturn(): string`, `witnessUnique(): 7`.
fn churn_module(cycle: usize) -> String {
    format!(
        "export function over(x: string): number;\n\
         export function over(x: number): string;\n\
         export function over(x: string | number): number | string {{ return x as any; }}\n\
         declare const both: ((x: string) => number) & ((x: number) => string);\n\
         export function pair<A, B>(a: A, b: B): [A, B] {{ return [a, b]; }}\n\
         export function tagged{cycle}(x: \"t{cycle}\"): {cycle} {{ return {cycle}; }}\n\
         export function witnessOverload() {{ return over(\"a\"); }}\n\
         export function witnessIntersection() {{ return both(1); }}\n\
         export function witnessGeneric() {{ return pair(\"a\", 1); }}\n\
         export function witnessReturnType() {{ const r: ReturnType<typeof over> = null as any; return r; }}\n\
         export function witnessParameters() {{ const p: Parameters<typeof over> = null as any; return p; }}\n\
         export function witnessIntersectionReturn() {{ const r: ReturnType<typeof both> = null as any; return r; }}\n\
         export function witnessUnique() {{ return tagged{cycle}(\"t{cycle}\"); }}\n"
    )
}

fn churn_upsert(host: &VerterHost, cycle: usize) {
    crate::u6_flow_shape_corpus_tests::upsert(
        host,
        CHURN,
        &crate::u6_flow_shape_corpus_tests::module_script(&churn_module(cycle)),
        crate::FileLanguage::script_ts(),
    );
}

/// Every witness's answer, reduced to the altitude the checker prints and
/// rendered structurally, so two hosts' answers compare.
fn observe(host: &VerterHost) -> Vec<String> {
    WITNESSES
        .iter()
        .map(|symbol| {
            let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::from(CHURN),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from(*symbol),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                },
                function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            };
            let carrier = host.get_flow_return_type_with_audit(
                &identity,
                crate::semantic_query::ReturnProjectionDemand::whole_return(),
            );
            let Ok(result) = carrier.as_result() else {
                return format!("{symbol}: refused");
            };
            let degraded = result.degradation().is_some();
            let store_view = host.resolver_store_view_read().into_owned_view();
            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
            let d = ProjectSemanticDispatch::new(&ctx);
            let node = d
                .normalize_node_keeping_declaration_refs_for_tests(
                    result.return_type(),
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                )
                .into_complete_node();
            let render = |node| match d.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Tuple { elements, .. }) => format!(
                    "[{}]",
                    elements
                        .iter()
                        .map(|element| render_node(&d, element.value, 1))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => render_node(&d, node, 0),
            };
            match node {
                Some(node) => format!(
                    "{symbol}: {}{}",
                    if degraded { "degraded " } else { "" },
                    render(node)
                ),
                None => format!("{symbol}: partial"),
            }
        })
        .collect()
}

/// A fresh host's answers for one version of the churned module.
fn fresh_observation(cycle: usize) -> Vec<String> {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    churn_upsert(&host, cycle);
    observe(&host)
}

/// Sustained UNIQUE edits of one module keep the kernel store bounded: the
/// edit path replaces the epoch once the store passes its record cap, so the
/// store never holds more than the cap plus one cycle's rebuild, although
/// the run interns far more than that in total. After every replacement the
/// answers equal a fresh host's — overloads, an intersection of callables,
/// a generic call, and `ReturnType` / `Parameters` over overloaded and
/// intersected callables — and a second replacement that retires only warm
/// state (no edit) changes no answer either.
///
/// Without the edit-path trigger the store grows by one cycle's records per
/// edit and passes the bound.
#[test]
fn sustained_unique_edits_keep_the_kernel_store_bounded_and_the_answers_fresh() {
    let host = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let store = graph.signature_store();

    churn_upsert(&host, 0);
    let first = observe(&host);
    assert_eq!(first, fresh_observation(0));
    for answer in &first {
        assert!(
            !answer.ends_with("refused") && !answer.ends_with("partial"),
            "premise: every witness answers: {first:?}"
        );
    }
    let working_set = store.interned_len();
    assert!(working_set > 0, "premise: the witnesses read the kernel");
    churn_upsert(&host, 1);
    let _ = observe(&host);
    let per_edit = store.interned_len() - working_set;
    assert!(per_edit > 0, "premise: a unique edit interns new records");
    let cap = store.interned_len() + 2 * per_edit;
    store.set_record_cap_for_tests(cap);

    let mut epoch = store.epoch();
    let (mut replacements, mut peak, mut interned) = (0usize, 0usize, 0usize);
    for cycle in 2..40 {
        let before = store.interned_len();
        churn_upsert(&host, cycle);
        let answers = observe(&host);
        let after = store.interned_len();
        peak = peak.max(after);
        if store.epoch() == epoch {
            interned += after - before;
            continue;
        }
        replacements += 1;
        interned += after;
        assert_eq!(
            answers,
            fresh_observation(cycle),
            "cycle {cycle}: the answers after a replacement differ from a fresh host's"
        );
        graph
            .compact_signature_store_over(0)
            .expect("a warm store is replaced");
        assert_eq!(
            observe(&host),
            answers,
            "cycle {cycle}: retiring warm kernel state changed an answer"
        );
        epoch = store.epoch();
    }
    assert!(replacements >= 3, "only {replacements} replacements");
    assert!(
        peak <= cap + working_set,
        "the store peaked at {peak} records past its cap {cap} plus one rebuild {working_set}"
    );
    assert!(
        interned > cap + working_set,
        "premise: an unreclaimed store would pass the bound ({interned} records interned)"
    );
}
