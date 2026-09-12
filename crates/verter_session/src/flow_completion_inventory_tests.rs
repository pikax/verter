//! The EXECUTABLE half of the completion-carrier inventory.
//!
//! The module next door makes a completion fact impossible to mint or read
//! without naming an inventory row. That closes the compile-time
//! direction: a new site cannot exist off the inventory. This suite closes
//! the two directions a compiler cannot see.
//!
//! - **Nothing on the list is prose.** Every construction and discharge row
//!   is VISITED by a real evaluation of a real program through the ordinary
//!   dispatch. A row that has drifted out of the pipeline — kept because
//!   deleting it looked risky, or written down ahead of the code — is
//!   caught by the set equality, not by a reviewer noticing.
//! - **Nothing off the list is cited.** The same equality catches a site
//!   citing a row the list does not carry, which is the exact shape of the
//!   carrier that gets MISSED: present in code, absent from the inventory.
//!
//! The probe table is why the equality means something. Each row names the
//! authored program whose published answer depends on that carrier holding
//! its fact, and the expected answer is the checker's own — so a carrier
//! whose fact is dropped fails its probe with a wrong TYPE, not merely with
//! a missing visit. The two halves discriminate different mutations:
//! dropping a carrier's fact breaks the probes, and removing the carrier
//! from the pipeline breaks the coverage.
//!
//! Every expected value is anchored against `tsc 7.0.2 --strict` through
//! `--declaration --emitDeclarationOnly`, which prints the inferred return
//! type directly.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::flow_completion_inventory::{
    coverage, CompletionConstruction, CompletionDischarge, CompletionTransport,
    FlowCompletionCarrier, FlowCompletionFact, FlowCompletionRole, TransportsCompletion,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnKey, QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr};

const PROBES: &str = "/ws/completioninventory/probes.ts";

/// One authored program per inventory row, with the checker's answer in
/// the table below.
const PROBES_SRC: &str = r#"
export function regionAccumulator(c: boolean) { if (c) { return 1 } return 0 }
export function synthesizedRegion() { return () => 1 }
export function bodyFromRootRegion(c: boolean) { if (c) return 1 }
export function evaluatorRefinement(k: "a" | "b") { switch (k) { case "a": return 1; case "b": return 0 } }
export function nestedBodyRefinement(c: boolean) { return () => { if (c) return 1 } }
export function switchCaseBreak(k: number) { switch (k) { case 1: return 1; default: break } }
export function recursiveRebuild(n: number) { if (n > 0) return recursiveRebuild(n - 1); return 1 }
export function memberOpenEnd(c: boolean) { if (c) return { b: 1 } }
export function memberClosedEnd() { return { b: 1 } }
export function freshLiteralWidening() { return 1 }
export function returnJoinVoid() { }
export function bareReturnOnly(c: boolean) { if (c) return; return }
"#;

fn lang(canonical: &str) -> crate::FileLanguage {
    crate::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution()
}

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(PROBES.to_string()),
        input_id: PROBES.to_string(),
        source: Arc::from(PROBES_SRC),
        file_language: lang(PROBES),
        aliases: Vec::new(),
    });
    host
}

/// The whole-return demand point.
fn whole_return_demand() -> crate::semantic_query::ReturnProjectionDemand {
    crate::semantic_query::ReturnProjectionDemand::whole_return()
}

/// The single-named-member demand point — the authored
/// `ReturnType<typeof f>['b']` shape.
fn member_demand(member: &str) -> crate::semantic_query::ReturnProjectionDemand {
    crate::semantic_query::ReturnProjectionDemand {
        point: {
            let mut point = crate::semantic_query::demand::Demand::identity();
            point.projection.path = crate::semantic_query::demand::ProjectionPath::from_segments([
                crate::semantic_query::PathSegment::Member(
                    crate::semantic_query::PropertyKey::identifier(member),
                ),
            ]);
            point
        },
    }
}

/// Execute one function's flow-return query through the ordinary dispatch.
/// `None` is a typed no-value outcome (a fail-closed refusal), not a panic.
fn evaluate(
    host: &Arc<VerterHost>,
    name: &str,
    demand: crate::semantic_query::ReturnProjectionDemand,
) -> Option<TypeExpr> {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(PROBES),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(PROBES),
        demand,
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract:
            crate::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
    };
    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => Some(
            host.project_node_to_type_expr_for_test(result.return_type())
                .expect("the flow-return value projects"),
        ),
        _ => None,
    }
}

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

fn number(value: f64) -> TypeExpr {
    TypeExpr::Literal(LiteralValue::Number(value))
}

fn union(arms: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Union(Arc::from(arms))
}

/// The RETURN type of the callable a probe publishes.
fn callable_return(published: &TypeExpr, name: &str) -> TypeExpr {
    match published {
        TypeExpr::Function(function) => function
            .return_type
            .as_deref()
            .cloned()
            .unwrap_or_else(|| panic!("{name} publishes a callable with an inferred return")),
        other => panic!("{name} must publish a callable, got {other:?}"),
    }
}

/// What a probe must observe.
enum Expected {
    /// The whole published return type.
    Whole(fn() -> TypeExpr),
    /// The return type of the callable the probe publishes.
    CallableReturn(fn() -> TypeExpr),
    /// A single-named-member demand that projects.
    Member(&'static str, fn() -> TypeExpr),
    /// A single-named-member demand that fails closed with no value.
    MemberFailsClosed(&'static str),
}

/// One row of the probe table.
struct Probe {
    /// The exported function this row evaluates.
    function: &'static str,
    /// What it must observe.
    expected: Expected,
    /// Why this program discriminates the carrier — what a dropped fact
    /// would publish instead.
    discriminates: &'static str,
}

const PROBE_TABLE: &[Probe] = &[
    Probe {
        function: "regionAccumulator",
        expected: Expected::Whole(|| union(vec![number(1.0), number(0.0)])),
        discriminates: "the consequent region ends its own path at the `return`, so the \
                        trailing return is a SECOND contributor; a region minted reachable \
                        would join `undefined`, and one minted unreachable would drop the \
                        trailing arm",
    },
    Probe {
        function: "synthesizedRegion",
        expected: Expected::CallableReturn(|| primitive(PrimitiveName::Number)),
        discriminates: "the returned arrow's expression body is a SYNTHESIZED region that \
                        cannot fall through, so its sole fresh literal widens; minted \
                        reachable it would publish `1 | undefined`",
    },
    Probe {
        function: "bodyFromRootRegion",
        expected: Expected::Whole(|| union(vec![number(1.0), primitive(PrimitiveName::Undefined)])),
        discriminates: "the body takes its fact from the root region — the guarded return \
                        leaves the end point reachable, so `undefined` joins and the literal \
                        does not widen",
    },
    Probe {
        function: "evaluatorRefinement",
        expected: Expected::Whole(|| union(vec![number(1.0), number(0.0)])),
        discriminates: "only the evaluator can see that the case tests EXHAUST the \
                        discriminant; without that refinement the no-matching-case path \
                        survives and joins `undefined`",
    },
    Probe {
        function: "nestedBodyRefinement",
        expected: Expected::CallableReturn(|| {
            union(vec![number(1.0), primitive(PrimitiveName::Undefined)])
        }),
        discriminates: "the NESTED body's own fact crosses into its own join — a dropped \
                        nested fact loses the arrow's `undefined` arm while the outer body \
                        keeps its own",
    },
    Probe {
        function: "switchCaseBreak",
        expected: Expected::Whole(|| union(vec![number(1.0), primitive(PrimitiveName::Undefined)])),
        discriminates: "the `default` clause's `break` is what reaches past the switch; \
                        without that fact a defaulted switch reads as ending every path and \
                        the sole literal widens to `number`",
    },
    Probe {
        function: "recursiveRebuild",
        expected: Expected::Whole(|| primitive(PrimitiveName::Number)),
        discriminates: "the recursive component's fixed point REBUILDS its member result \
                        each round and must carry the completion fact across the rebuild; a \
                        rebuild that restored reachability would join `undefined`",
    },
    Probe {
        function: "memberOpenEnd",
        expected: Expected::MemberFailsClosed("b"),
        discriminates: "a member demand over a body that can still fall through has no \
                        modeled point — the missing `undefined` arm would have to be folded \
                        into the member access — so it must refuse rather than answer",
    },
    Probe {
        function: "memberClosedEnd",
        expected: Expected::Member("b", || primitive(PrimitiveName::Number)),
        discriminates: "the same demand over a body that cannot fall through DOES project; \
                        this is the positive control that proves the refusal above is the \
                        completion fact talking and not the member demand itself",
    },
    Probe {
        function: "freshLiteralWidening",
        expected: Expected::Whole(|| primitive(PrimitiveName::Number)),
        discriminates: "a sole fresh literal return widens ONLY when no fall-through arm \
                        joins it; a fact read as reachable publishes `1 | undefined`",
    },
    Probe {
        function: "returnJoinVoid",
        expected: Expected::Whole(|| primitive(PrimitiveName::Void)),
        discriminates: "an arm-less body that CAN fall through is `void`; the join reads the \
                        fact to choose between that and the authored-form seed",
    },
    Probe {
        function: "bareReturnOnly",
        expected: Expected::Whole(|| primitive(PrimitiveName::Void)),
        discriminates: "a body whose only contributions are bare `return;` statements is \
                        `void` — the observation pair must carry the bare-return fact \
                        separately from the fall-through one",
    },
];

/// Run one probe and assert its observation.
fn run(host: &Arc<VerterHost>, probe: &Probe) {
    match &probe.expected {
        Expected::Whole(expected) => assert_eq!(
            evaluate(host, probe.function, whole_return_demand()),
            Some(expected()),
            "{}: {}",
            probe.function,
            probe.discriminates
        ),
        Expected::CallableReturn(expected) => {
            let published = evaluate(host, probe.function, whole_return_demand())
                .unwrap_or_else(|| panic!("{} must publish a value", probe.function));
            assert_eq!(
                callable_return(&published, probe.function),
                expected(),
                "{}: {}",
                probe.function,
                probe.discriminates
            );
        }
        Expected::Member(member, expected) => assert_eq!(
            evaluate(host, probe.function, member_demand(member)),
            Some(expected()),
            "{}: {}",
            probe.function,
            probe.discriminates
        ),
        Expected::MemberFailsClosed(member) => assert_eq!(
            evaluate(host, probe.function, member_demand(member)),
            None,
            "{}: {}",
            probe.function,
            probe.discriminates
        ),
    }
}

/// Every probe observes the checker's answer.
///
/// This is the half that makes the coverage assertion below mean
/// something: a carrier can only be VISITED with the right fact if the
/// value it decided is right too.
#[test]
pub(crate) fn every_probe_observes_the_checkers_answer() {
    let host = host();
    for probe in PROBE_TABLE {
        run(&host, probe);
    }
}

/// The listed inventory and the cited inventory are the same set.
///
/// Set EQUALITY, in both directions, is the whole point. A listed row no
/// evaluation reaches has drifted out of the pipeline and is prose again;
/// a cited row the list does not carry is the missed carrier the inventory
/// exists to catch. Transport rows are types rather than sites, so they are
/// bound separately by [`transport_rows_bind_to_their_carrier_types`].
#[test]
pub(crate) fn every_site_row_is_reached_and_every_reached_row_is_listed() {
    let host = host();
    let (_, visited) = coverage::record(|| {
        for probe in PROBE_TABLE {
            run(&host, probe);
        }
        // The hermetic fixtures mint a completion fact for a body they
        // never lowered. They are carriers like any other, so the
        // recording exercises them here rather than exempting them.
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let graph = dispatch.graph();
        let node = graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let _ = crate::for_tests::flow_return_result_for_tests(graph, node);
        let _ = crate::for_tests::degraded_flow_return_result_for_tests(graph, node);
    });

    let listed: BTreeSet<FlowCompletionCarrier> = CompletionConstruction::ALL
        .iter()
        .copied()
        .map(FlowCompletionCarrier::Construction)
        .chain(
            CompletionDischarge::ALL
                .iter()
                .copied()
                .map(FlowCompletionCarrier::Discharge),
        )
        .collect();

    // The role rides the diagnostic: knowing WHICH stage lost a carrier is
    // what turns a failure here into a place to look.
    let describe = |rows: &[FlowCompletionCarrier]| {
        rows.iter()
            .map(|row| format!("{row:?} ({:?})", row.role()))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let unreached: Vec<_> = listed.difference(&visited).copied().collect();
    assert!(
        unreached.is_empty(),
        "inventory rows no probe reaches — each is either prose that drifted in or a carrier \
         the probe table stopped exercising: {}",
        describe(&unreached)
    );

    let unlisted: Vec<_> = visited.difference(&listed).copied().collect();
    assert!(
        unlisted.is_empty(),
        "carriers cited by real evaluations that the inventory does not list: {}",
        describe(&unlisted)
    );
}

/// Every transport row names the real type that stores the fact.
///
/// The match is EXHAUSTIVE, so a new [`CompletionTransport`] variant does
/// not compile until a type claims it through the sealed
/// [`TransportsCompletion`] impl — which is what keeps the row list
/// code-first rather than a name someone wrote down.
#[test]
fn transport_rows_bind_to_their_carrier_types() {
    fn bound<T: TransportsCompletion>(row: CompletionTransport) {
        assert_eq!(
            T::TRANSPORT,
            row,
            "a transport row must name the type that claims it"
        );
    }
    for row in CompletionTransport::ALL.iter().copied() {
        match row {
            CompletionTransport::SliceRegion => {
                bound::<crate::flow_slice_content::SliceRegion>(row);
            }
            CompletionTransport::SliceContent => {
                bound::<crate::flow_slice_content::SliceContent>(row);
            }
            CompletionTransport::SliceSwitchCase => {
                bound::<crate::flow_slice_content::SliceSwitchCase>(row);
            }
            CompletionTransport::BodyCompletionObservations => {
                bound::<crate::flow_completion_inventory::BodyCompletionObservations>(row);
            }
            CompletionTransport::FlowReturnResult => {
                bound::<crate::semantic_query::FlowReturnResult>(row);
            }
        }
    }
}

/// The inventory covers every stage of a fact's life, and every fact has a
/// carrier.
///
/// The debt this inventory closes was scoped as "producers, transient
/// carriers, constructions, transfers, discharges, result assembly,
/// publication and admission exits". A row set that silently lost a whole
/// stage — no transfer left, nothing at the admission exit — would still
/// satisfy the equality above while describing a pipeline that no longer
/// exists, so the stages themselves are asserted present.
#[test]
fn the_inventory_covers_every_stage_of_a_facts_life() {
    let mut roles: BTreeSet<FlowCompletionRole> = BTreeSet::new();
    let mut facts: BTreeSet<FlowCompletionFact> = BTreeSet::new();
    for site in CompletionConstruction::ALL.iter().copied() {
        roles.insert(site.role());
        facts.insert(site.fact());
    }
    for site in CompletionDischarge::ALL.iter().copied() {
        roles.insert(site.role());
        facts.insert(site.fact());
    }
    for site in CompletionTransport::ALL.iter().copied() {
        roles.insert(site.role());
        facts.extend(site.facts().iter().copied());
    }

    assert_eq!(
        roles,
        BTreeSet::from([
            FlowCompletionRole::Producer,
            FlowCompletionRole::TransientCarrier,
            FlowCompletionRole::Construction,
            FlowCompletionRole::Transfer,
            FlowCompletionRole::Discharge,
            FlowCompletionRole::ResultAssembly,
            FlowCompletionRole::Publication,
            FlowCompletionRole::AdmissionExit,
        ]),
        "the inventory must cover every stage of a completion fact's life"
    );

    assert_eq!(
        facts,
        BTreeSet::from([
            FlowCompletionFact::NormalCompletion,
            FlowCompletionFact::BareReturn,
            FlowCompletionFact::ImplicitUndefined,
            FlowCompletionFact::AuthoredForm,
            FlowCompletionFact::SwitchCaseBreak,
        ]),
        "every completion fact must have at least one carrier on the inventory"
    );

    assert_eq!(
        FlowCompletionCarrier::all().len(),
        CompletionConstruction::ALL.len()
            + CompletionDischarge::ALL.len()
            + CompletionTransport::ALL.len(),
        "the assembled inventory must be exactly the three vocabularies"
    );
}
