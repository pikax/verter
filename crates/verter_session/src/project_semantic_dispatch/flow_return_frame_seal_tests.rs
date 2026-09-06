//! @ai-generated - the POSITIONAL/FRAME boundary, sealed by type.
//!
//! A sub-expression the substrate cannot model is a POSITION. A frame
//! fails only for a reason that is genuinely about the whole frame: an
//! unmodelled CONTROL surface, a missing body, a budget, an empty cycle,
//! a torn view, an unmodelled demand point.
//!
//! Every row asserts on the GRAPH NODE, pins the typed degradation and
//! the slot candidate count, and names the checker's answer.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;

const SEAL_CANONICAL: &str = "/ws/flow-frame-seal.ts";

const SEAL_FIXTURE: &str = r#"
export class Box { readonly tag = "box"; }

// ── the callee rail: a marker in a callee's RETURN position ──────────
//
// tsgo: `{ label: string; made: Box }`
export function q1LocalHelperBare() {
  const f = () => new Box();
  return { label: "x", made: f() };
}

// tsgo: `{ label: string; made: (string | Box)[] }`
export function q1LocalHelperArray() {
  const f = () => ["s", new Box()];
  return { label: "x", made: f() };
}

// tsgo: `{ label: string; made: (string | Box)[] }`
export function q1IifeArray() {
  return { label: "x", made: (() => ["s", new Box()])() };
}

// ── a nested body whose CONTROL surface is unmodelled ────────────────
//
// tsgo: `{ label: string; go: (n: number) => number }`
export function objWithLoopArrow() {
  return {
    label: "x",
    go: (n: number) => {
      while (n > 0) {
        return n;
      }
      return 0;
    },
  };
}

// ── a hoisted nested function declaration read as a callee ───────────
//
// tsgo: `{ label: string; made: number }`
export function localFunctionShadowCall() {
  function g() {
    return 1;
  }
  return { label: "x", made: g() };
}

// ── item 6: the ARRAY element granularity ────────────────────────────
//
// tsgo: `{ label: string; made: (string | Box)[] }`
export function arrayWithUnmodeledElement() {
  return { label: "x", made: ["s", new Box()] };
}

// ── the CLEAN control: nothing degraded, warm ────────────────────────
export function cleanControl() {
  return { label: "x", n: 1 };
}
"#;

fn make_seal_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(SEAL_CANONICAL.to_string()),
        input_id: SEAL_CANONICAL.to_string(),
        source: Arc::from(SEAL_FIXTURE),
        file_language: crate::LanguageRegistry::global()
            .classify_static(SEAL_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn with_dispatch<R>(
    host: &Arc<VerterHost>,
    f: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R,
) -> R {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    f(&dispatch)
}

fn key_for(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(SEAL_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(SEAL_CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    }
}

struct Outcome {
    node: SemanticNodeId,
    degradation: Option<FlowReturnDegradation>,
    candidates: usize,
}

fn evaluate(host: &Arc<VerterHost>, name: &str) -> Option<Outcome> {
    with_dispatch(host, |dispatch| {
        let key = key_for(dispatch, name);
        let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::FlowReturn(result),
                ..
            }) => result,
            _ => return None,
        };
        let candidates = dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
        Some(Outcome {
            node: result.return_type(),
            degradation: result.degradation(),
            candidates,
        })
    })
}

#[track_caller]
fn member(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    key: &str,
) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            view.positive_members()
                .iter()
                .find(|member| member.key.as_string() == Some(key))
                .unwrap_or_else(|| panic!("member `{key}` must be present on {node:?}"))
                .value
        }
        other => panic!("expected an Object graph node, got {other:?}"),
    }
}

#[track_caller]
fn assert_marker(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId, what: &str) {
    assert!(
        matches!(
            dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::UnmodeledPosition
            ))
        ),
        "{what}: the POSITIONAL marker carrier (never `Miss`, never a fabricated \
         `any`), got {:?}",
        dispatch.graph().node_data(node)
    );
}

#[track_caller]
fn assert_string_label(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId, what: &str) {
    let label = member(dispatch, node, "label");
    assert!(
        matches!(
            dispatch.graph().node_data(label).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                | Some(SemanticNodeData::Literal(_))
        ),
        "{what}: the MODELLED sibling survives, got {:?}",
        dispatch.graph().node_data(label)
    );
}

/// A CALLEE whose own body left one position unmodelled still HAS a return
/// value, and the caller keeps it.
///
/// The positional marker used to be spelled `Opaque(QueryError::Miss)` —
/// the exact node the callee rail's signature reader matches to answer
/// `SignatureCall::ReturnMiss`, which the call arms turned back into a
/// frame-level `Err`. So a marker minted one frame down fed itself
/// straight into the whole-frame failure it exists to avoid, and three
/// programs the PARENT commit answered stopped producing a value at all:
///
/// | program | tsgo `7.0.0-dev.20260526.1` |
/// |---|---|
/// | `const f = () => new Box(); return { label: "x", made: f() }` | `{ label: string; made: Box }` |
/// | `const f = () => ["s", new Box()]; return { label: "x", made: f() }` | `{ label: string; made: (string \| Box)[] }` |
/// | `return { label: "x", made: (() => ["s", new Box()])() }` | same |
///
/// The parent's answer was not right either: the first two published
/// `Array<string \| any>` with NO degradation and WARM — a fabricated
/// `any` inside a clean result. The target is neither: the composite
/// survives, the unmodelled slot carries the typed marker, and nothing
/// admits.
///
/// Mutation recipe: minting the marker as `QueryError::Miss` again makes
/// `of_signature_node` classify it `ReturnMiss`, and all three rows lose
/// their `label`.
#[test]
fn a_marker_in_a_callee_return_position_is_a_value_not_a_frame_failure() {
    let host = make_seal_host();
    for (name, degradation) in [
        (
            "q1LocalHelperBare",
            FlowReturnDegradation::UnmodeledPosition,
        ),
        (
            "q1LocalHelperArray",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
        (
            "q1IifeArray",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
    ] {
        let outcome =
            evaluate(&host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
        with_dispatch(&host, |dispatch| {
            assert_string_label(dispatch, outcome.node, name);
            assert_marker(dispatch, member(dispatch, outcome.node, "made"), name);
        });
        assert_eq!(
            outcome.degradation,
            Some(degradation),
            "{name} carries the positional degradation reason"
        );
        assert_eq!(
            outcome.candidates, 0,
            "{name} is a degraded success — ReturnOnly, nothing warms"
        );
    }
}

/// A NESTED function value whose own body has an unmodelled CONTROL
/// surface keeps its signature; only its RETURN position carries the
/// marker.
///
/// The nested body's frame-level failure used to propagate through the
/// enclosing frame's `?` (`let contributors = contributors?`), deleting
/// the whole enclosing object. tsgo types
/// `{ label: "x", go: (n: number) => { while (n > 0) { return n } return 0 } }`
/// as `{ label: string; go: (n: number) => number }`; the loop is beyond
/// this substrate, so `go`'s RETURN is the marker — and `go` is still a
/// one-parameter call signature, and `label` is still `string`.
///
/// Mutation recipe: restoring the `?` collapses the object and the `label`
/// lookup fails with "expected an Object graph node".
#[test]
fn a_nested_bodys_control_surface_failure_marks_its_return_not_the_enclosing_frame() {
    let host = make_seal_host();
    let outcome = evaluate(&host, "objWithLoopArrow").expect("objWithLoopArrow produces a value");
    with_dispatch(&host, |dispatch| {
        assert_string_label(dispatch, outcome.node, "objWithLoopArrow");
        let go = member(dispatch, outcome.node, "go");
        match dispatch.graph().node_data(go).as_deref() {
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) => {
                assert_eq!(params.len(), 1, "the nested signature keeps its parameter");
                assert_marker(dispatch, *return_type, "objWithLoopArrow.go return");
            }
            other => panic!("`go` publishes the nested signature, got {other:?}"),
        }
    });
    assert_eq!(
        outcome.degradation,
        Some(FlowReturnDegradation::UnmodeledPosition)
    );
    assert_eq!(outcome.candidates, 0);
}

/// A CALL FORM the substrate does not model is one position: a hoisted
/// nested function declaration read as a callee.
///
/// `SliceCall::LocalFunctionShadow` returned a frame-level `Err`, so
/// `function outer() { function g() { return 1 } return { label: "x",
/// made: g() } }` published nothing at all. tsgo:
/// `{ label: string; made: number }`. Recovering the nested declaration's
/// own return is downstream work; publishing NOTHING for the enclosing
/// object is not the same fact.
#[test]
fn an_unmodeled_call_form_marks_its_position_only() {
    let host = make_seal_host();
    let outcome =
        evaluate(&host, "localFunctionShadowCall").expect("localFunctionShadowCall has a value");
    with_dispatch(&host, |dispatch| {
        assert_string_label(dispatch, outcome.node, "localFunctionShadowCall");
        assert_marker(
            dispatch,
            member(dispatch, outcome.node, "made"),
            "localFunctionShadowCall",
        );
    });
    assert_eq!(
        outcome.degradation,
        Some(FlowReturnDegradation::UnmodeledPosition)
    );
    assert_eq!(outcome.candidates, 0);
}

/// The CLEAN control: an ordinary body still answers cleanly and warms
/// exactly one candidate.
///
/// Every row above asserts an absence. This one asserts the presence a
/// blanket "mark everything unmodelled" fix would destroy.
#[test]
fn the_clean_control_is_undegraded_and_warms() {
    let host = make_seal_host();
    let outcome = evaluate(&host, "cleanControl").expect("cleanControl produces a value");
    assert_eq!(outcome.degradation, None, "the clean control is undegraded");
    assert_eq!(
        outcome.candidates, 1,
        "a clean result warm-admits exactly one candidate"
    );
    with_dispatch(&host, |dispatch| {
        assert_string_label(dispatch, outcome.node, "cleanControl");
        assert!(
            matches!(
                dispatch
                    .graph()
                    .node_data(member(dispatch, outcome.node, "n"))
                    .as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Number))
            ),
            "the clean control's second member is fully modelled"
        );
    });
}

/// CHARACTERIZATION, not an endorsement: an unmodelled ELEMENT collapses
/// the whole ARRAY to one marker.
///
/// `return { label: "x", made: ["s", new Box()] }` — tsgo
/// `7.0.0-dev.20260526.1` types `made` as `(string | Box)[]`. This
/// substrate publishes a BARE marker for `made`, losing the modelled
/// `string` element: the positional rule holds at the OBJECT level
/// (`label` survives) but NOT inside the array, because an array literal
/// has no structural carrier in the slice content at all — it is lowered
/// as one leaf, and the leaf gate refuses the whole answer when it embeds
/// a fabricated `any`.
///
/// The granularity is OWED, not settled. Fixing it is a coordinated
/// change to the SHARED value-descent classifier
/// (`verter_semantic::analysis::flow::value_descent`), the demand PLANNER
/// (which must select element value spans, or a structural array's
/// elements lower as `SliceExpr::Elided` and are lost), the content half,
/// and the evaluator — the two halves must gain the same disposition in
/// one change, exactly as `ValueDescent::Object` did. A content-only
/// structural array would disagree with the planner about SELECTION.
///
/// The row is asserted so the gap cannot drift silently, and so the
/// no-wrong-and-warm half stays pinned: whatever the granularity, the
/// result is DEGRADED and admits nothing.
#[test]
fn an_unmodeled_array_element_collapses_the_array_and_is_owed() {
    let host = make_seal_host();
    let outcome = evaluate(&host, "arrayWithUnmodeledElement")
        .expect("arrayWithUnmodeledElement produces a value");
    with_dispatch(&host, |dispatch| {
        assert_string_label(dispatch, outcome.node, "arrayWithUnmodeledElement");
        let made = member(dispatch, outcome.node, "made");
        // THE OWED SHAPE is `Array { element: String | MARKER }`. Today it
        // is the bare marker; the assertion states which one is live so
        // the owning change flips exactly this line.
        assert_marker(dispatch, made, "arrayWithUnmodeledElement");
        assert!(
            !matches!(
                dispatch.graph().node_data(made).as_deref(),
                Some(SemanticNodeData::Array { .. })
            ),
            "if this now interns an Array, the granularity landed — update the row \
             and the owed shape above"
        );
    });
    // The half that is NOT owed: no wrong answer, and nothing warms.
    assert_eq!(
        outcome.degradation,
        Some(FlowReturnDegradation::FlowGap(
            crate::semantic_query::FlowGap::UnmodeledExpression
        ))
    );
    assert_eq!(outcome.candidates, 0);
}

// ────────────────────────────────────────────────────────────────────────
// The frame's PRODUCT state: the evidence a discharge rests on, the
// determinism of the state the merges produce, and the budget boundary a
// merge runs under.
// ────────────────────────────────────────────────────────────────────────

/// Frames whose semantic state the product domains actually carry: a
/// binding whose slot facts a discharge must rest on, a branch whose
/// merge exercises the frame join, and a mutual component whose members
/// publish together.
const PRODUCT_CANONICAL: &str = "/ws/flow-products.ts";

const PRODUCT_FIXTURE: &str = r#"
export function boundControl(c: boolean) {
  const k = 1;
  if (c) {
    return k;
  }
  return 2;
}

export function branchJoin(c: boolean) {
  let v: string | number = "s";
  if (c) {
    v = 1;
  }
  return v;
}

export function switchJoin(c: number) {
  let v: string | number = "s";
  switch (c) {
    case 1:
      v = 1;
      break;
    default:
      break;
  }
  return v;
}

export function tryJoin(c: boolean) {
  let v: string | number = "s";
  try {
    v = 1;
  } catch (e) {
    v = "t";
  }
  return v;
}

export function scBoundA(c: boolean) {
  const k = 1;
  if (c) return k;
  return scBoundB(c);
}

export function scBoundB(c: boolean) {
  const m = 2;
  if (c) return m;
  return scBoundA(c);
}
"#;

/// Two frames with the SAME body and different signature ARITY.
///
/// The body returns out of a `try`, so the `finally` boundary merges the
/// pending return edge into the entering state through the frame join —
/// and completing a return-edge snapshot's parameter layer files one
/// product per SIGNATURE parameter, read or unread. The wide frame
/// therefore reaches that merge holding far more subjects than the demand
/// plan's selection names, while the narrow one does not. Generated
/// rather than written out because the arity is the whole variable: the
/// two bodies must be identical for the comparison to mean anything.
const WIDE_SIGNATURE_ARITY: usize = 256;

fn signature_arity_fixtures() -> String {
    let mut source = String::new();
    // bounded-loop: the two declared fixture arities.
    for (name, arity) in [
        ("narrowSignatureJoin", 2),
        ("wideSignatureJoin", WIDE_SIGNATURE_ARITY),
    ] {
        let mut params = String::from("c: boolean");
        // bounded-loop: the fixture's own declared arity.
        for ordinal in 1..arity {
            params.push_str(&format!(", p{ordinal}: string"));
        }
        source.push_str(&format!(
            "\nexport function {name}({params}) {{\n  const k = 1;\n  try {{\n    if (c) {{\n      return k;\n    }}\n  }} finally {{\n    c;\n  }}\n  return 2;\n}}\n"
        ));
    }
    source
}

fn make_product_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(PRODUCT_CANONICAL.to_string()),
        input_id: PRODUCT_CANONICAL.to_string(),
        source: Arc::from(format!("{PRODUCT_FIXTURE}{}", signature_arity_fixtures())),
        file_language: crate::LanguageRegistry::global()
            .classify_static(PRODUCT_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn product_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(PRODUCT_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(PRODUCT_CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    }
}

/// One evaluated demand: the structural type it served (arena-free, so
/// two hosts are comparable), the slot candidate count, and how many cold
/// computations the run performed.
struct ProductRun {
    served: Option<verter_type_expr::TypeExpr>,
    candidates: usize,
    cold_computes: u32,
}

/// Serve `name` `demands` times, EACH through a fresh store view — the
/// warm read of a published candidate runs against a view where the cold
/// build's artifacts are visible — under ONE request context, so the
/// cold-compute counter measures the whole run.
fn run_product(host: &Arc<VerterHost>, name: &str, demands: u32) -> ProductRun {
    use crate::request_context::{RequestContext, RequestContextGuard};
    let ctx = RequestContext::new(1, Arc::from(PRODUCT_CANONICAL), false, None);
    let _guard = RequestContextGuard::install(ctx);
    let mut served = None;
    // bounded-loop: the caller-supplied demand count.
    for _ in 0..demands {
        served = with_dispatch(host, |dispatch| {
            let key = product_key(dispatch, name);
            match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => host.project_node_to_type_expr_for_test(result.return_type()),
                _ => None,
            }
        });
    }
    let cold_computes = crate::request_context::current_request_context()
        .expect("the run installs a RequestContext")
        .flow_return_cold_computes
        .load(std::sync::atomic::Ordering::Relaxed);
    let candidates = with_dispatch(host, |dispatch| {
        let key = product_key(dispatch, name);
        dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
    });
    ProductRun {
        served,
        candidates,
        cold_computes,
    }
}

/// A demand's completeness rests on the frame's PRODUCT evidence: a
/// planned binding obligation discharges only when the evaluation
/// actually produced that binding's definite-assignment product.
///
/// Both legs run the SAME otherwise-clean evaluation. The control admits
/// warm — one candidate, one cold compute across two demands. With one
/// required binding-domain product dropped from an otherwise untouched
/// witness (the walk ledger, the call evidence and the convergence log
/// all stay clean), the binding obligation stays unclaimed, no
/// `CompleteFlowResult` mints, and BOTH demands recompute cold with zero
/// candidates — at the root and at SCC publication alike, since both
/// finalize through the one discharge report.
///
/// The fault is refuse-only: it can withhold evidence, never mint it. A
/// green control beside a red injected leg therefore proves the seal
/// discriminates missing product evidence rather than riding the
/// report's say-so.
#[test]
fn flow_discharge_requires_product_evidence() {
    use super::flow_return::flow_admission_fault_injection as inject;

    // Root leg: the control warms.
    let host = make_product_host();
    let control = run_product(&host, "boundControl", 2);
    assert!(
        control.served.is_some(),
        "the control produces a usable value"
    );
    assert_eq!(
        control.candidates, 1,
        "a clean bound frame warm-admits exactly one candidate"
    );
    assert_eq!(
        control.cold_computes, 1,
        "the second demand of a clean bound frame is a warm hit"
    );

    // Root leg: the same evaluation without one binding-domain product.
    let injected_host = make_product_host();
    let injected = {
        let _drop_product = inject::Guard::arm(
            &injected_host
                .flow_fault_injection
                .drop_binding_domain_product,
        );
        run_product(&injected_host, "boundControl", 2)
    };
    assert_eq!(
        injected.served, control.served,
        "the evaluated value is unchanged — only the discharge evidence is"
    );
    assert_eq!(
        injected.candidates, 0,
        "a binding obligation with no product evidence never mints a proof: \
         zero candidates"
    );
    assert_eq!(
        injected.cold_computes, 2,
        "an unproven demand never warms, so the second demand recomputes cold"
    );

    // SCC leg: a component member's missing product keeps the WHOLE batch
    // out of the publish set.
    let scc_control = make_product_host();
    let scc_control_run = run_product(&scc_control, "scBoundA", 1);
    assert!(
        scc_control_run.served.is_some(),
        "the component control produces a usable value"
    );
    let scc_injected = make_product_host();
    let _drop_product = inject::Guard::arm(
        &scc_injected
            .flow_fault_injection
            .drop_binding_domain_product,
    );
    with_dispatch(&scc_injected, |dispatch| {
        let root = product_key(dispatch, "scBoundA");
        let peer = product_key(dispatch, "scBoundB");
        let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(root.clone())));
        for (name, key) in [("scBoundA", root), ("scBoundB", peer)] {
            assert_eq!(
                dispatch
                    .graph()
                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
                0,
                "{name}: a member without product evidence never enters the SCC \
                 publish batch"
            );
        }
    });
}

/// The frame's product state — and therefore the served result and its
/// warm candidate — does not depend on the order equivalent demands
/// arrive in.
///
/// Two hosts evaluate the same fixture through DIFFERENT but equivalent
/// request orders: one takes the merging frame first, the other takes it
/// last. Request order decides which semantic nodes are interned first,
/// so it decides their ids — and the product join's contributor
/// aggregation must not let that reach the answer. The served type, the
/// candidate count, and the cold-compute count must agree exactly under
/// either order.
#[test]
fn flow_product_worklist_is_permutation_deterministic() {
    let forward = make_product_host();
    let forward_first = run_product(&forward, "switchJoin", 2);
    let forward_second = run_product(&forward, "boundControl", 2);

    let reverse = make_product_host();
    let reverse_second = run_product(&reverse, "boundControl", 2);
    let reverse_first = run_product(&reverse, "switchJoin", 2);

    // The merging frame's answer is the JOIN of both dispatch edges, not
    // one edge's: the assertion the order-independence above is about.
    for (order, run) in [("forward", &forward_first), ("reverse", &reverse_first)] {
        let Some(verter_type_expr::TypeExpr::Union(arms)) = run.served.as_ref() else {
            panic!(
                "{order}: the merging frame serves the union of both dispatch \
                 edges, got {:?}",
                run.served
            );
        };
        assert_eq!(
            arms.len(),
            2,
            "{order}: both incoming edges contribute to the merged reaching type"
        );
    }

    for (name, a, b) in [
        ("switchJoin", &forward_first, &reverse_first),
        ("boundControl", &forward_second, &reverse_second),
    ] {
        assert_eq!(
            a.served, b.served,
            "{name}: an equivalent request order serves the same value"
        );
        assert_eq!(
            a.candidates, b.candidates,
            "{name}: an equivalent request order admits the same candidate"
        );
        assert_eq!(
            a.cold_computes, b.cold_computes,
            "{name}: an equivalent request order performs the same cold work"
        );
    }
}

/// The product budget is the demand plan's OWN policy, and exhausting it
/// is a typed frame failure that retains nothing.
///
/// The control frame merges within its plan's convergence policy and
/// warm-admits. Under a ZERO-iteration budget the very first frame merge
/// exhausts the policy, so the merge produces no joined state, the frame
/// reports the typed budget failure, and the demand retains NO candidate
/// and recomputes cold on the next request.
#[test]
fn flow_product_budget_boundary_is_exact_and_never_warm() {
    use super::flow_return::flow_admission_fault_injection as inject;

    let host = make_product_host();
    let control = run_product(&host, "switchJoin", 2);
    assert!(
        control.served.is_some(),
        "the control frame merges within its plan's convergence policy"
    );
    assert_eq!(
        control.candidates, 1,
        "a converged frame warm-admits exactly one candidate"
    );
    assert_eq!(
        control.cold_computes, 1,
        "the second demand of a converged frame is a warm hit"
    );

    let exhausted_host = make_product_host();
    let _zero_budget = inject::Guard::arm(
        &exhausted_host
            .flow_fault_injection
            .zero_product_iteration_budget,
    );
    let exhausted = run_product(&exhausted_host, "switchJoin", 2);
    assert_eq!(
        exhausted.candidates, 0,
        "an exhausted product budget retains no candidate"
    );
    assert_eq!(
        exhausted.cold_computes, 2,
        "a budget-exhausted demand recomputes cold rather than serving a \
         retained partial"
    );
}

/// The frame's product budget counts BOTH subject spaces, and the
/// parameter space is not selection-bound.
///
/// `max_products` is derived from the demand plan, so it must cover every
/// subject the frame can hold. A binding subject is minted at a
/// slice-selected site, but a PARAMETER subject is not: completing a
/// return-edge snapshot's parameter layer files one product per signature
/// parameter whether the body reads it or not. A budget derived from the
/// selection alone therefore exhausts at the first merge that sees a
/// completed parameter layer, and the frame stops producing a value at
/// all: the typed budget failure is what refuses its admission, so the
/// demand recomputes it cold and fails again on every request.
///
/// Both frames here have the SAME body; only the arity differs. The wide
/// one must converge, serve the same value, and warm exactly like the
/// narrow one.
#[test]
fn a_wide_signature_frame_converges_within_its_own_product_budget() {
    let narrow_host = make_product_host();
    let narrow = run_product(&narrow_host, "narrowSignatureJoin", 2);
    let wide_host = make_product_host();
    let wide = run_product(&wide_host, "wideSignatureJoin", 2);

    assert!(
        narrow.served.is_some(),
        "the narrow control frame converges and serves a value"
    );
    assert_eq!(
        wide.served, narrow.served,
        "the unread parameters do not change the value the frame serves"
    );
    assert_eq!(
        narrow.candidates, 1,
        "the narrow control warm-admits exactly one candidate"
    );
    assert_eq!(
        wide.candidates, narrow.candidates,
        "a frame whose parameter subjects outnumber its selection still \
         converges and warm-admits exactly one candidate"
    );
    assert_eq!(
        wide.cold_computes, narrow.cold_computes,
        "the second demand of the wide frame is a warm hit, exactly like \
         the control's — a budget that ignored the parameter space exhausts \
         at this frame's merge, and nothing it produces can be admitted"
    );
}
