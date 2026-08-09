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
    for name in ["q1LocalHelperBare", "q1LocalHelperArray", "q1IifeArray"] {
        let outcome =
            evaluate(&host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
        with_dispatch(&host, |dispatch| {
            assert_string_label(dispatch, outcome.node, name);
            assert_marker(dispatch, member(dispatch, outcome.node, "made"), name);
        });
        assert_eq!(
            outcome.degradation,
            Some(FlowReturnDegradation::UnmodeledPosition),
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
        Some(FlowReturnDegradation::UnmodeledPosition)
    );
    assert_eq!(outcome.candidates, 0);
}
