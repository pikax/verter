//! The production polled semantic entry on a single-threaded WASM host. A
//! root whose evaluation meets another task's producer returns `Pending`
//! instead of parking the only thread; once that producer goes away, the
//! same entry delivers, in the query's own value domain, what the
//! synchronous entry serves for the same key.
use super::host;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::fact_validation_port::FactValidation as _;
use verter_type_engine::resolver_core::ResolverCapabilities;
use verter_type_engine::semantic_query::{
    CacheRead, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId,
    SemanticNodeData, SemanticQueryKey, SemanticQueryValue, ValueRootKey,
};
use verter_type_engine::semantic_query_memo::{Claim, ReadCapture, SemanticGraphStore};
use wasm_bindgen_test::wasm_bindgen_test;

const CHAIN_FILE: &str = "/semantic-poll/chain.ts";

/// A chain of constants each initialized from the one before: the `typeof`
/// of the last is a chain of continuation frames.
const CHAIN: &str = "const x0 = \"ok\" as const;\n\
                     const x1 = x0;\n\
                     const x2 = x1;\n\
                     const x3 = x2;\n\
                     export {};\n";

/// Counts its wakes.
#[derive(Default)]
struct CountingWaker(AtomicUsize);

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn chain_host() -> host::VerterHost {
    let host = host::VerterHost::new_standalone(host::HostConfig::default());
    let _ = host
        .upsert(host::UpsertRequest {
            canonical_id: Some(CHAIN_FILE.into()),
            input_id: CHAIN_FILE.into(),
            source: Arc::from(CHAIN),
            file_language: host::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the in-memory source is accepted");
    host
}

/// A root the tests poll, with the key another task holds while they do.
#[derive(Clone, Copy)]
enum Root {
    /// The `typeof` of the last constant of the chain, while another task
    /// produces the `typeof` of a constant it reaches.
    Chain,
    /// The truthiness classification of `string` — a query off the
    /// continuation runtime answering outside the type-node domain — while
    /// another task produces that same classification.
    Classification,
    /// A conditional whose check reads the constant chain.
    Conditional,
    /// A distributed conditional whose final union reduction is held.
    Distribution,
    /// `defineProps` over two type arguments, owned by the chain's file,
    /// whose payload intersection is held.
    MacroPayload,
}

fn key_for<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    root: Root,
    held: bool,
) -> SemanticQueryKey {
    match root {
        Root::Chain => dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    CHAIN_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: verter_type_expr::OwnedStr::from(if held { "x1" } else { "x3" }),
            },
            ProjectionReductionContext::published(ProjectionMode::Navigate),
        ),
        Root::Conditional if held => dispatch.typeof_key_for(
            ValueRootKey {
                scope: ScopeId::file(
                    CHAIN_FILE,
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: "x3".into(),
            },
            ProjectionReductionContext::structural_transit(),
        ),
        Root::Conditional => {
            let graph = dispatch.graph_arc();
            let check = graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId::file(
                        CHAIN_FILE,
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    ),
                    name: "x3".into(),
                },
                Default::default(),
                Arc::from([]),
            ));
            let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            SemanticQueryKey::Conditional {
                check,
                extends: string,
                true_branch: number,
                false_branch: string,
                distributive: false,
                pending: None,
            }
        }
        Root::Classification => SemanticQueryKey::ClassifyTruthinessDomain {
            subject: dispatch
                .graph_arc()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
        },
        Root::MacroPayload => {
            let graph = dispatch.graph_arc();
            let args: Arc<[verter_type_engine::semantic_query::SemanticNodeId]> = Arc::from([
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
            ]);
            if held {
                SemanticQueryKey::reduce_intersection_operands(graph, args)
            } else {
                SemanticQueryKey::ResolveMacroPayload {
                    owner:
                        verter_type_engine::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                            Arc::<str>::from(CHAIN_FILE),
                            verter_type_expr::TopLevelOwnerId::instance(0),
                            Arc::<str>::from("<sfc-script-setup>"),
                        ),
                    macro_index: 0,
                    macro_kind: verter_session_query::analysis::types::AnalyzedMacroKind::DefineProps,
                    type_args: args,
                    context: verter_type_engine::semantic_query::MacroPayloadContext::new(
                        Default::default(),
                        ProjectionMode::Expanded,
                    ),
                }
            }
        }
        Root::Distribution => {
            let graph = dispatch.graph_arc();
            let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            if held {
                SemanticQueryKey::ReduceUnion {
                    members: Arc::from([string, number]),
                    nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
                }
            } else {
                let boolean =
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
                let yes = graph.intern_node(SemanticNodeData::Literal(
                    verter_type_engine::semantic_query::LiteralValue::Boolean(true),
                ));
                SemanticQueryKey::Conditional {
                    check: boolean,
                    extends: yes,
                    true_branch: string,
                    false_branch: number,
                    distributive: true,
                    pending: None,
                }
            }
        }
    }
}

/// What a read answered and its fact status and rails.
fn summary(read: &CacheRead<QueryResult<SemanticQueryValue>>) -> (String, String, bool, bool) {
    (
        format!("{:?}", read.value()),
        format!("{:?}", read.fact_status()),
        read.cache_suppress(),
        read.result_is_partial(),
    )
}

/// Poll `root` on this, the only, thread while another task produces its
/// held key: the entry yields, the producer going away wakes it, and its
/// next poll delivers. Returns the delivered read, summarized, with what
/// the synchronous entry serves for the same key afterwards.
fn poll_while_another_task_produces(root: Root) -> [(String, String, bool, bool); 2] {
    let host = chain_host();
    let mut dispatch = ProjectSemanticDispatch::new(&host);
    let _builder = dispatch.enter_construction_scope();
    let graph: &SemanticGraphStore = host.project_type_store().semantic_graph();
    let other = graph.task_registry_for_tests().register_task();
    let held = key_for(&dispatch, root, true);
    let mut held_keys = vec![held.clone()];
    if let SemanticQueryKey::ReduceUnion {
        members,
        nullability,
    } = held
    {
        held_keys.push(SemanticQueryKey::ReduceUnion {
            members: members.iter().copied().rev().collect(),
            nullability,
        });
    }
    let leases: Vec<_> = held_keys
        .into_iter()
        .map(|key| {
            let mut capture = ReadCapture::default();
            let Ok(mut attempt) =
                graph.begin_query_claim(&host, host.request_flags(), key, &mut capture)
            else {
                panic!("a cold key has no warm answer");
            };
            let Claim::Produce(lease) = graph.claim_query(
                &host,
                host.request_flags(),
                &mut attempt,
                &other,
                &mut capture,
            ) else {
                panic!("the other task produces the cold key");
            };
            lease
        })
        .collect();
    let counter = Arc::new(CountingWaker::default());
    let waker = Waker::from(Arc::clone(&counter));
    let key = key_for(&dispatch, root, false);
    let mut entry = dispatch.poll_query(key.clone());
    let first = entry.poll(&waker);
    drop(leases);
    assert!(
        first.is_pending(),
        "the root yields instead of parking the only thread: {:?}",
        match &first {
            Poll::Ready(read) => Some(read.value()),
            Poll::Pending => None,
        }
    );
    assert!(
        counter.0.load(Ordering::SeqCst) >= 1,
        "the producer going away wakes the entry"
    );
    let Poll::Ready(polled) = entry.poll(&waker) else {
        panic!("the woken entry produces what it waited on itself");
    };
    drop(entry);
    assert_eq!(graph.wait_graph_counts_for_tests().1, 0);
    drop(other);
    let synchronous = dispatch.execute_via_cold_build_helper(key);
    [summary(&polled), summary(&synchronous)]
}

#[wasm_bindgen_test]
fn semantic_poll_contract_a_chain_waiting_on_another_producer_yields_on_the_only_thread() {
    let [polled, synchronous] = poll_while_another_task_produces(Root::Chain);
    assert!(
        polled.0.contains("TypeNode"),
        "the chain answers a type: {polled:?}"
    );
    assert_eq!(polled, synchronous);
}

#[wasm_bindgen_test]
fn semantic_poll_contract_a_root_off_the_runtime_yields_on_the_only_thread() {
    let [polled, synchronous] = poll_while_another_task_produces(Root::Classification);
    assert!(
        polled.0.contains("TruthinessDomain"),
        "the classification answers in its own value domain: {polled:?}"
    );
    assert_eq!(polled, synchronous);
}

#[wasm_bindgen_test]
fn semantic_poll_contract_a_conditional_dependency_yields_on_the_only_thread() {
    let [polled, synchronous] = poll_while_another_task_produces(Root::Conditional);
    assert!(polled.0.contains("TypeNode"), "the decision answers a type");
    assert_eq!(polled, synchronous);
}

#[wasm_bindgen_test]
fn semantic_poll_contract_a_distributed_reduction_yields_on_the_only_thread() {
    let [polled, synchronous] = poll_while_another_task_produces(Root::Distribution);
    assert!(
        polled.0.contains("TypeNode"),
        "the distribution answers a type"
    );
    assert!(
        !polled.2 && !polled.3,
        "the distribution is complete and reusable"
    );
    assert_eq!(polled, synchronous);
}

#[wasm_bindgen_test]
fn semantic_poll_contract_a_macro_payload_continuation_yields_on_the_only_thread() {
    let [polled, synchronous] = poll_while_another_task_produces(Root::MacroPayload);
    assert!(
        polled.0.contains("TypeNode"),
        "the payload answers a type: {polled:?}"
    );
    assert!(
        !polled.2 && !polled.3,
        "the payload is complete and reusable"
    );
    assert_eq!(polled, synchronous);
}

const UNLOADED_FILE: &str = "/semantic-poll/unloaded.ts";

/// A host whose workspace holds [`UNLOADED_FILE`], unloaded.
fn unloaded_host() -> host::VerterHost {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        UNLOADED_FILE.to_owned(),
        Arc::from("export interface Props { a: string }\n"),
    );
    host::VerterHost::new(
        host::HostConfig {
            analysis_level: host::AnalysisLevel::Full,
            ..host::HostConfig::default()
        },
        workspace,
    )
}

/// The instantiation of the `Props` [`UNLOADED_FILE`] declares.
fn unloaded_props_key<C: ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
) -> SemanticQueryKey {
    SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
        dispatch.type_slot_for(
            UNLOADED_FILE,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Props",
        ),
        Arc::from([]),
        dispatch.instantiate_context_for(
            UNLOADED_FILE,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        ),
    ))
}

/// An instantiation whose declaring file is not loaded: on the only
/// thread, the polled entry waits on the file's load before the build
/// begins and, polled again whenever it is pending, delivers within a
/// bounded number of polls what the synchronous entry serves on a fresh
/// host.
#[wasm_bindgen_test]
fn semantic_poll_contract_an_instantiation_of_an_unloaded_file_loads_on_the_only_thread() {
    let synchronous = {
        let host = unloaded_host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        summary(&dispatch.execute_via_cold_build_helper(unloaded_props_key(&dispatch)))
    };
    assert!(
        synchronous.0.contains("TypeNode") && !synchronous.3,
        "the instantiation answers a complete type: {synchronous:?}"
    );
    let host = unloaded_host();
    let mut dispatch = ProjectSemanticDispatch::new(&host);
    let waker = Waker::from(Arc::new(CountingWaker::default()));
    let mut entry = dispatch.poll_query(unloaded_props_key(&dispatch));
    let polled = (0..64)
        .find_map(|_| match entry.poll(&waker) {
            Poll::Ready(read) => Some(read),
            Poll::Pending => None,
        })
        .expect("the load completes on the only thread within bounded polls");
    drop(entry);
    assert_eq!(summary(&polled), synchronous);
}
