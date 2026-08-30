//! @ai-generated - POSITIONAL non-modelling regression tests for the
//! demand-sliced `FlowReturn` evaluator.
//!
//! ONE invariant class: a sub-expression the substrate has no model for
//! is a POSITION, not a frame. The enclosing structure keeps every
//! sibling it did model, the unmodelled position carries the typed
//! unresolved MARKER, and the whole result is a DEGRADED SUCCESS —
//! usable, never warm.
//!
//! Two failures this file characterises:
//!
//! - a composite (an object literal / an array / an enclosing call) with
//!   ONE unmodelled member must not collapse the whole composite. The
//!   frame-level `Err` that made whole-frame propagation the `?` default
//!   published `[]` for `defineProps<ReturnType<typeof makeProps>>()`
//!   where the checker publishes `{ label: string; made: Box }`;
//! - a value that is NOT unresolved must not be reported unresolved. A
//!   4,100-arm literal union with zero misses tripped a construction-time
//!   node budget and was stamped a factually false `UnresolvedValue`,
//!   which then propagated as a permanent warm refusal into every
//!   enclosing result.
//!
//! Every row asserts on the GRAPH NODE, never the projected `TypeExpr`,
//! and pins the degradation reason plus the slot candidate count.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;

const POS_CANONICAL: &str = "/ws/flow-positional.ts";

/// Every unmodelled position below sits INSIDE a composite that also
/// carries at least one fully-modelled sibling. The sibling is the
/// discriminator: a fix that merely stops fabricating a value, without
/// making the position local, deletes it.
const POS_FIXTURE: &str = r#"
export class Box { readonly tag = "box"; }

// ── B-F1: one unmodelled member inside an object literal ─────────────
export function objectWithUnmodeledCall() {
  return { label: "x", made: new Box() };
}

// The byte-equivalent local-binding spelling — already survived at HEAD
// through `FailedBindingInitializer`, and is the control that proves the
// disposition must not depend on where the evaluator was standing.
export function objectWithUnmodeledLocal() {
  const b = new Box();
  return { label: "x", made: b };
}

// ── the same rule over the OTHER positional variant ──────────────────
export function objectWithUnmodeledBinding() {
  class Local { readonly k = 1; }
  return { label: "x", made: Local };
}

// ── B-F2: an array element at a call position ────────────────────────
//
// The composite that survives here is the OBJECT, not the array: `made`
// collapses to ONE marker rather than `Array<string | MARKER>`, losing
// the modelled `"s"` element (tsgo: `{ label: string; made: (string |
// Box)[] }`). That collapse is a KNOWN OWED granularity gap, NOT the
// rule this file states — characterized, with its owner, by
// `flow_return_frame_seal_tests::an_unmodeled_array_element_collapses_the_array_and_is_owed`.
export function arrayWithUnmodeledCall() {
  return { label: "x", made: ["s", new Box()] };
}

// ── A-F2 / A-F3: the residual warm-fabricated-`any` call forms ───────
export declare function fs(): string;
export function assignmentCallPosition(z: string) {
  return { label: "x", made: (z = fs()) };
}
export function computedMemberOffCall() {
  return { label: "x", made: fs()["q"] };
}
export function optionalCallMemberRead() {
  return { label: "x", made: fs?.()?.length };
}
export function binaryOverCall() {
  return { label: "x", made: fs() + "y" };
}

// ── the COMPOSITE TWIN of every other positional class ───────────────
//
// Each row is the SAME unmodelled position the whole-return fail-closed
// rows exercise, placed inside an object literal that also carries a
// fully modelled `label`. The sibling is the discriminator: a fix that
// only stops fabricating a value, without making the position LOCAL,
// deletes it.
export class Info { readonly kind = "info" }

// A frame-shadowed LEAF answer whose name the owner scope also answers.
export function twinFrameShadowedLeaf() {
  class Info { static s = 1 }
  return { label: "x", made: Info.s };
}

// A DECLARATOR ANNOTATION naming a frame-owned type the owner scope also
// answers.
export function twinDeclaratorAnnotation() {
  interface Info { local: true }
  const v: Info = { local: true };
  return { label: "x", made: v };
}

// A NESTED SIGNATURE parameter annotation naming a frame-owned type.
export function twinNestedParamAnnotation() {
  interface Info { local: true }
  return { label: "x", made: (p: Info) => p };
}

// A NESTED SIGNATURE type-parameter CONSTRAINT naming a frame-owned type.
export function twinNestedConstraint() {
  interface Info { local: true }
  return { label: "x", made: <U extends Info>(p: U) => p };
}

// A local `enum` read as a value — the `UnmodeledBinding` content
// carrier, in a composite position.
export function twinLocalEnum() {
  enum E { A = 1 }
  return { label: "x", made: E };
}

// ── A-F1: a wide literal union with ZERO unresolved carriers ─────────
export type Wide =
  | "s0" | "s1" | "s2" | "s3" | "s4" | "s5" | "s6" | "s7" | "s8" | "s9";
export function wideUnionPassthrough(x: Wide) {
  return x;
}
"#;

/// The generated 4,100-arm twin: the union is authored programmatically
/// so the row measures the ARM COUNT axis rather than a fixture edit.
fn wide_union_source(arms: usize) -> String {
    // The union is INLINE on the parameter annotation, never behind a
    // named alias: a `DeclRef` carrier is a shallow stop, so an aliased
    // union never reaches the value structure the verdict is taken over
    // and cannot measure this axis at all.
    let mut source = String::from("export function wideUnionPassthrough(x:\n");
    for index in 0..arms {
        source.push_str(&format!("  | \"s{index}\"\n"));
    }
    source.push_str(") {\n  return x;\n}\n");
    source
}

const WIDE_CANONICAL: &str = "/ws/flow-wide-union.ts";

fn make_host(canonical: &str, source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn make_pos_host() -> Arc<VerterHost> {
    make_host(POS_CANONICAL, POS_FIXTURE)
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

fn key_for(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    }
}

/// One evaluated flow-return outcome: the whole-return GRAPH NODE, the
/// typed degradation, and the slot candidate count.
struct Outcome {
    node: SemanticNodeId,
    degradation: Option<FlowReturnDegradation>,
    candidates: usize,
}

#[track_caller]
fn evaluate(host: &Arc<VerterHost>, canonical: &str, name: &str) -> Option<Outcome> {
    with_dispatch(host, |dispatch| {
        let key = key_for(dispatch, canonical, name);
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

/// The named member of an `Object` graph node — asserted on the node, so
/// a collapsed composite cannot hide behind a projection.
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
fn is_unresolved_marker(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> bool {
    matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::Opaque(error)) if error.means_type_is_not_yet_known()
    )
}

/// A composite with ONE unmodelled member keeps every member it DID
/// model, marks the unmodelled one, and warms nothing.
///
/// This is B-F1 at the evaluator boundary. The checker's answer for
/// `objectWithUnmodeledCall` is `{ label: string; made: Box }`; the
/// substrate cannot type `new Box()` (that is `U6.CALL_RESOLVE`), so
/// `made` is the typed unresolved marker — never a fabricated `any`,
/// which is indistinguishable from an authored one at every downstream
/// gate, and never a discarded composite.
///
/// Mutation recipe: routing the unmodelled position back through a
/// frame-level `Err` collapses the whole object and the `label`
/// assertion fails with "expected an Object graph node".
#[test]
fn an_unmodeled_member_marks_its_position_and_the_composite_survives() {
    let host = make_pos_host();
    // `assignmentCallPosition` writes to a PARAMETER, so the evaluation
    // observes `UnappliedWriteEffect` first and first-observed wins. The
    // load-bearing assertions for that row are the surviving sibling, the
    // marker, and the zero candidate count — the reason is a control on
    // the OTHER rows, where nothing else is degraded.
    for (name, reason) in [
        (
            "objectWithUnmodeledCall",
            FlowReturnDegradation::UnmodeledPosition,
        ),
        (
            "objectWithUnmodeledBinding",
            FlowReturnDegradation::UnmodeledPosition,
        ),
        (
            "arrayWithUnmodeledCall",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
        (
            "assignmentCallPosition",
            FlowReturnDegradation::UnappliedWriteEffect,
        ),
        (
            "computedMemberOffCall",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
        (
            "optionalCallMemberRead",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
        (
            "binaryOverCall",
            FlowReturnDegradation::FlowGap(crate::semantic_query::FlowGap::UnmodeledExpression),
        ),
    ] {
        let outcome = evaluate(&host, POS_CANONICAL, name)
            .unwrap_or_else(|| panic!("{name} must produce a value"));
        with_dispatch(&host, |dispatch| {
            let label = member(dispatch, outcome.node, "label");
            assert!(
                matches!(
                    dispatch.graph().node_data(label).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                        | Some(SemanticNodeData::Literal(_))
                ),
                "{name}: the MODELLED sibling must survive, got {:?}",
                dispatch.graph().node_data(label)
            );
            let made = member(dispatch, outcome.node, "made");
            assert!(
                is_unresolved_marker(dispatch, made),
                "{name}: the unmodelled position must carry the typed marker, got {:?}",
                dispatch.graph().node_data(made)
            );
        });
        assert_eq!(
            outcome.degradation,
            Some(reason),
            "{name} must carry its typed degradation reason"
        );
        assert_eq!(
            outcome.candidates, 0,
            "{name} degraded success is ReturnOnly — nothing warms"
        );
    }
}

/// The COMPOSITE-POSITION TWIN of every other positional class.
///
/// The whole-return fail-closed rows in `flow_return_lexical_tests` /
/// `flow_return_root_gate_tests` each place their unmodelled position at
/// the WHOLE RETURN, where "the composite survives" has nothing to
/// observe — which is why five rounds of fixes could route positional
/// non-modelling through a frame-level `Err` and stay green. Every row
/// here is the same position with a modelled sibling beside it.
///
/// Mutation recipe: restoring a frame-level `Err` at ANY of the converted
/// sites collapses that row's object and the `label` lookup fails with
/// "expected an Object graph node"; the whole-return rows stay green.
#[test]
fn every_positional_class_survives_inside_a_composite() {
    let host = make_pos_host();
    for name in [
        "twinFrameShadowedLeaf",
        "twinDeclaratorAnnotation",
        "twinNestedParamAnnotation",
        "twinNestedConstraint",
        "twinLocalEnum",
    ] {
        let outcome = evaluate(&host, POS_CANONICAL, name)
            .unwrap_or_else(|| panic!("{name} must produce a value"));
        with_dispatch(&host, |dispatch| {
            let label = member(dispatch, outcome.node, "label");
            assert!(
                matches!(
                    dispatch.graph().node_data(label).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                        | Some(SemanticNodeData::Literal(_))
                ),
                "{name}: the MODELLED sibling must survive, got {:?}",
                dispatch.graph().node_data(label)
            );
            let made = member(dispatch, outcome.node, "made");
            assert!(
                dispatch.graph().node_reaches_unresolved(made),
                "{name}: the unmodelled position REACHES the typed marker \
                 (at the slot, or inside the signature composed around it), got {:?}",
                dispatch.graph().node_data(made)
            );
        });
        assert_eq!(
            outcome.degradation,
            Some(FlowReturnDegradation::UnmodeledPosition),
            "{name} carries the positional degradation reason"
        );
        assert_eq!(
            outcome.candidates, 0,
            "{name} degraded success is ReturnOnly — nothing warms"
        );
    }
}

/// The byte-equivalent LOCAL-BINDING spelling reaches the same
/// disposition. Two programs that mean the same thing must not differ by
/// where the evaluator was standing when it met the unmodelled call.
#[test]
fn the_local_binding_spelling_reaches_the_same_disposition() {
    let host = make_pos_host();
    let outcome = evaluate(&host, POS_CANONICAL, "objectWithUnmodeledLocal")
        .expect("objectWithUnmodeledLocal must produce a value");
    with_dispatch(&host, |dispatch| {
        let label = member(dispatch, outcome.node, "label");
        assert!(
            matches!(
                dispatch.graph().node_data(label).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                    | Some(SemanticNodeData::Literal(_))
            ),
            "the modelled sibling must survive"
        );
    });
    assert_eq!(
        outcome.candidates, 0,
        "the local-binding spelling is ReturnOnly too"
    );
    assert!(
        outcome.degradation.is_some(),
        "the local-binding spelling carries a typed degradation"
    );
}

/// A wide literal union with ZERO unresolved carriers is CLEAN and WARM
/// at every arm count.
///
/// A-F1: the construction-time value walk was bounded by a 4,096-node
/// budget and reported "unresolved" on exhaustion. A 4,100-arm union with
/// no miss in it is fully known, so stamping it `UnresolvedValue` is
/// factually false — and the falsehood propagates as a permanent warm
/// refusal into every enclosing result.
///
/// Mutation recipe: reintroducing any node budget whose exhaustion
/// returns "unresolved" flips the 4,100-arm row to
/// `Some(UnresolvedValue)` / 0 candidates while leaving the 10-arm row
/// green — which is exactly why both counts are asserted.
#[test]
fn a_wide_union_with_no_miss_is_clean_and_warm_at_every_arm_count() {
    for arms in [10_usize, 4100] {
        let host = make_host(WIDE_CANONICAL, &wide_union_source(arms));
        let outcome = evaluate(&host, WIDE_CANONICAL, "wideUnionPassthrough")
            .unwrap_or_else(|| panic!("{arms} arms must produce a value"));
        assert_eq!(
            outcome.degradation, None,
            "{arms} arms: a union with zero unresolved carriers is CLEAN"
        );
        assert_eq!(
            outcome.candidates, 1,
            "{arms} arms: a clean result warm-admits exactly one candidate"
        );
        with_dispatch(&host, |dispatch| {
            assert!(
                !is_unresolved_marker(dispatch, outcome.node),
                "{arms} arms: a fully-known union is never the unresolved marker, got {:?}",
                dispatch.graph().node_data(outcome.node)
            );
        });
    }
}
