//! @ai-generated - FlowReturn substrate characterization rows.
//!
//! Two families of rows:
//!
//! - Un-ignored `flow_surface_*` / characterization rows pin the
//!   observable FlowReturn surface: symbolic call returns resolve, the
//!   `this`-call fallback yields `any`, return-free loops stay fall-through
//!   transparent, return-bearing loop/switch/try stay degraded, an empty
//!   recursive cycle degrades without collapsing to `never`, and complete
//!   unannotated functions never surface a semantic miss.
//!
//! - `flow_return_substrate_*` rows are the producer-routing contracts:
//!   each asserts the pinned surface AND that the demand was served by a
//!   `FlowReturn` dispatch through `ProjectSemanticDispatch`.

use super::support::*;
use crate::VerterHost;
use verter_audit::RequestKindPayload;
use verter_type_expr::LiteralValue;

const SUBSTRATE: &str = "/fixtures/flow_return_substrate.ts";

/// Dispatch-mask bit of the `FlowReturn` family — its
/// [`crate::semantic_query::SemanticQueryKeyTag::bit_index`].
fn flow_return_dispatch_bit() -> u32 {
    crate::semantic_query::SemanticQueryKeyTag::FlowReturn.bit_index()
}

fn upsert_substrate_fixture(host: &VerterHost) {
    upsert_ts(host, SUBSTRATE, FLOW_RETURN_SUBSTRATE);
}

fn resolve_substrate_alias(
    host: &VerterHost,
    alias: &str,
) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    resolve_expr(host, SUBSTRATE, alias, &[], ProjectionMode::Expanded)
}

/// Whether this request dispatched the `FlowReturn` family anywhere in its
/// resolution (root or any nested subquery).
fn flow_return_dispatched(record: &verter_audit::RequestAuditRecord) -> bool {
    match &record.kind_payload {
        RequestKindPayload::TypeResolution(payload) => {
            payload.semantic_query_dispatch_mask & (1 << flow_return_dispatch_bit()) != 0
        }
        other => panic!("expected TypeResolution payload, got {other:?}"),
    }
}

fn assert_flow_return_dispatched(record: &verter_audit::RequestAuditRecord, alias: &str) {
    assert!(
        flow_return_dispatched(record),
        "{alias} must be served by a FlowReturn dispatch through ProjectSemanticDispatch"
    );
}

/// The degraded surface: a composed signature with no recoverable return
/// carrier projects the typed miss.
fn assert_semantic_miss(expr: &TypeExpr) {
    match expr {
        TypeExpr::Unknown(unknown) => assert_eq!(unknown.raw(), "semanticMiss"),
        other => panic!("expected the degraded semantic-miss surface, got {other:?}"),
    }
}

fn expr_contains_semantic_miss(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Unknown(unknown) => unknown.raw() == "semanticMiss",
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(expr_contains_semantic_miss)
        }
        TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) => expr_contains_semantic_miss(&prop.ty),
            verter_type_expr::ObjectMember::Method(method) => method
                .function
                .return_type
                .as_deref()
                .is_some_and(expr_contains_semantic_miss),
            _ => false,
        }),
        TypeExpr::Array { element, .. } => expr_contains_semantic_miss(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| expr_contains_semantic_miss(&element.ty)),
        TypeExpr::Function(function) => function
            .return_type
            .as_deref()
            .is_some_and(expr_contains_semantic_miss),
        TypeExpr::Ref { type_arguments, .. } => {
            type_arguments.iter().any(expr_contains_semantic_miss)
        }
        TypeExpr::Parenthesized(inner) => expr_contains_semantic_miss(inner),
        _ => false,
    }
}

fn expr_contains_never(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(PrimitiveName::Never) => true,
        TypeExpr::Union(members) | TypeExpr::Intersection(members) => {
            members.iter().any(expr_contains_never)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Current-behavior characterization (GREEN; must keep holding)
// ---------------------------------------------------------------------------

#[test]
fn flow_surface_symbolic_call_return_resolves_complete() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubCallReturn");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn flow_surface_return_free_loop_stays_fallthrough_transparent() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubCallAfterLoop");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

/// A `this.helper()` call FAILS CLOSED at the flow surface.
///
/// `this` is not modeled (the receiver capability is separate work), so
/// the call has no structural arm: the shared shallow pass answers it
/// with a bare `any` that carries no call-return carrier. Publishing
/// that `any` was a fabricated value at a call position — clean, warm,
/// and wrong (tsgo `7.0.0-dev.20260526.1` types `SubThisCall#run` as
/// `number`). The classifier now decides the call position on the FORM,
/// so this joins the return-bearing-loop / `switch` rows above.
#[test]
fn flow_surface_this_call_return_fails_closed() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubThisCallRun");
    assert_semantic_miss(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn flow_surface_return_bearing_loop_is_degraded_not_narrowed() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubLoopReturn");
    assert_semantic_miss(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn flow_surface_switch_return_is_degraded_not_narrowed() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubSwitchReturn");
    assert_semantic_miss(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn flow_surface_try_return_is_degraded_not_narrowed() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubTryReturn");
    assert_semantic_miss(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn empty_recursive_cycle_degrades_without_collapsing_to_never() {
    // Approved behavior pin (both halves): the cycle is degraded (no
    // admission) and it must NEVER collapse to `never`.
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubEmptyRecursion");
    assert!(
        !expr_contains_never(&expr),
        "an empty recursive cycle must never produce `never`, got {expr:?}"
    );
    assert_semantic_miss(&expr);
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn unannotated_complete_functions_never_surface_a_semantic_miss() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    for alias in [
        "SubCallReturn",
        "SubCompleteUnion",
        "SubCompleteFallthrough",
    ] {
        let (expr, _) = resolve_substrate_alias(&host, alias);
        assert!(
            !expr_contains_semantic_miss(&expr),
            "{alias} is a complete unannotated function and must not surface a semantic miss: {expr:?}"
        );
    }
    let (union, _) = resolve_substrate_alias(&host, "SubCompleteUnion");
    let TypeExpr::Union(_) = &union else {
        panic!("SubCompleteUnion must stay a union surface, got {union:?}");
    };
}

#[test]
fn mixed_relation_function_return_component_stays_coinductive_assignable() {
    // Relate(SubMixedA, SubMixedB) → the `next` return relation →
    // SubMixedA.next's body-derived return → Relate(SubMixedB, SubMixedA) →
    // assumption on the open relation. The component closes coinductive
    // positive; this verdict must survive the obligation-runtime rewrite.
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubMixedAssign");
    assert_string_literal(&expr, "yes");
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

// ---------------------------------------------------------------------------
// Producer-routing contracts (RED → lifted as the substrate lands)
// ---------------------------------------------------------------------------

#[test]
fn flow_return_substrate_serves_symbolic_call_return_complete() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubCallReturn");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_flow_return_dispatched(&record, "SubCallReturn");
}

#[test]
fn flow_return_substrate_fails_closed_on_a_this_call() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubThisCallRun");
    assert_semantic_miss(&expr);
    assert_flow_return_dispatched(&record, "SubThisCallRun");
}

#[test]
fn flow_return_substrate_serves_return_free_loop_transparent() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubCallAfterLoop");
    let props = object_props(&expr);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_flow_return_dispatched(&record, "SubCallAfterLoop");
}

#[test]
fn flow_return_substrate_keeps_return_bearing_loop_degraded() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubLoopReturn");
    assert_semantic_miss(&expr);
    assert_flow_return_dispatched(&record, "SubLoopReturn");
}

#[test]
fn flow_return_substrate_keeps_switch_degraded() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubSwitchReturn");
    assert_semantic_miss(&expr);
    assert_flow_return_dispatched(&record, "SubSwitchReturn");
}

#[test]
fn flow_return_substrate_keeps_try_degraded() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubTryReturn");
    assert_semantic_miss(&expr);
    assert_flow_return_dispatched(&record, "SubTryReturn");
}

#[test]
fn flow_return_substrate_empty_cycle_admits_nothing_and_never_produces_never() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubEmptyRecursion");
    assert!(
        !expr_contains_never(&expr),
        "an empty recursive cycle must never produce `never`, got {expr:?}"
    );
    assert_semantic_miss(&expr);
    assert_flow_return_dispatched(&record, "SubEmptyRecursion");
}

#[test]
fn flow_return_substrate_base_plus_recursion_admits_widened_number() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubBaseRecursion");
    assert_primitive(&expr, PrimitiveName::Number);
    assert_flow_return_dispatched(&record, "SubBaseRecursion");
}

#[test]
fn flow_return_substrate_signature_raise_and_return_type_share_one_producer() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    // Signature raise: `typeof subShared` materializes the function's
    // signature, demanding its body-derived return.
    let (typeof_expr, typeof_record) = evaluate_expr(
        &host,
        SUBSTRATE,
        "typeof subShared",
        ProjectionMode::Expanded,
    );
    let signature = function_type(&typeof_expr);
    let return_ty = signature
        .return_type
        .as_deref()
        .expect("typeof subShared must carry a return type");
    let props = object_props(return_ty);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_flow_return_dispatched(&typeof_record, "typeof subShared");

    // ReturnType<typeof f> over the same function demands the same return.
    let (alias_expr, alias_record) = resolve_substrate_alias(&host, "SubCallerA");
    let props = object_props(&alias_expr);
    assert_eq!(prop_names(&props), vec!["ok"]);
    assert_primitive(&props["ok"].ty, PrimitiveName::String);
    assert_flow_return_dispatched(&alias_record, "SubCallerA");
}

#[test]
fn flow_return_substrate_value_environment_cannot_enter_type_substitution() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    // Two callers with disjoint local value environments demand the SAME
    // callee return; both are served by the one callee FlowReturn identity.
    let (a_expr, a_record) = resolve_substrate_alias(&host, "SubCallerA");
    let (b_expr, b_record) = resolve_substrate_alias(&host, "SubCallerB");
    assert_eq!(a_expr, b_expr);
    assert_flow_return_dispatched(&a_record, "SubCallerA");
    assert_flow_return_dispatched(&b_record, "SubCallerB");
    // A repeat demand revalidates warm without fresh source work.
    let (warm_expr, warm_record) = resolve_substrate_alias(&host, "SubCallerB");
    assert_eq!(warm_expr, b_expr);
    assert_no_fresh_source_loading(&warm_record);
}

#[test]
fn flow_return_substrate_mixed_scc_records_flow_frame_inside_relation() {
    let host = make_host_with_footprint();
    upsert_substrate_fixture(&host);
    let (expr, record) = resolve_substrate_alias(&host, "SubMixedAssign");
    assert_string_literal(&expr, "yes");
    assert_flow_return_dispatched(&record, "SubMixedAssign");
}

/// Same-file MUTUAL recursion discharges coinductively on the concrete
/// seed: `mutA` contributes the concrete `1` (widened to `number`); the
/// `mutA -> mutB -> mutA` back-edge is a hold, and `mutB`'s hold-only
/// outcome upgrades at the SCC close to its target's admitted return.
/// Mutation recipe: mapping the in-flight back-edge to a failure (or
/// poisoning the component on `mutB`'s empty cycle) flips either side to
/// `semanticMiss` — the clean `number` both-sides pin discriminates the
/// coinductive discharge.
#[test]
fn flow_return_substrate_mutual_recursion_discharges_on_the_concrete_seed() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function mutA(c: boolean) {
  if (c) return 1;
  return mutB(c);
}
export function mutB(c: boolean) {
  return mutA(c);
}
export type MutAFirst = ReturnType<typeof mutA>;
export type MutBFirst = ReturnType<typeof mutB>;
"#,
    );
    // a-first: the inline member (mutB) is empty-cycle at pop and upgrades
    // at the close.
    let (expr_a, _) = resolve_expr(&host, SUBSTRATE, "MutAFirst", &[], ProjectionMode::Expanded);
    assert_primitive(&expr_a, PrimitiveName::Number);
    // b-first (warm a): the root discharges on the inline member's seed.
    let (expr_b, _) = resolve_expr(&host, SUBSTRATE, "MutBFirst", &[], ProjectionMode::Expanded);
    assert_primitive(&expr_b, PrimitiveName::Number);
}

/// An empty MUTUAL cycle (holds only, no concrete seed anywhere) is
/// `ReturnOnly` for the whole component — nothing admits, and nothing
/// collapses to `never`.
#[test]
fn flow_return_substrate_empty_mutual_cycle_admits_nothing() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function cycA() {
  return cycB();
}
export function cycB() {
  return cycA();
}
export type Cyc = ReturnType<typeof cycA>;
"#,
    );
    let (expr, _) = resolve_expr(&host, SUBSTRATE, "Cyc", &[], ProjectionMode::Expanded);
    assert!(
        !expr_contains_never(&expr),
        "an empty mutual cycle must never produce `never`, got {expr:?}"
    );
    assert_semantic_miss(&expr);
}

/// A method inside a RETURNED object literal evaluates its body-derived
/// return through the nested function value (no body scan): the outer
/// function's return publishes the method's signature with its inferred
/// return. Mutation recipe: treating method members as non-structural
/// (the leaf fallback) renders `m`'s return a semantic miss.
#[test]
fn flow_return_substrate_returned_object_method_return_evaluates() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function outer() {
  return { m() { return 1 as const } };
}
export type OuterMethodReturn = ReturnType<ReturnType<typeof outer>["m"]>;
"#,
    );
    let (expr, _) = resolve_expr(
        &host,
        SUBSTRATE,
        "OuterMethodReturn",
        &[],
        ProjectionMode::Expanded,
    );
    assert_number_literal(&expr, 1.0);
}

/// A nested block-bodied arrow value evaluates its body-derived return
/// through the nested function value (no body scan).
#[test]
fn flow_return_substrate_nested_block_arrow_return_evaluates() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function outerArrow() {
  return (() => { return 1; })();
}
export type OuterIifeReturn = ReturnType<typeof outerArrow>;
"#,
    );
    let (expr, _) = resolve_expr(
        &host,
        SUBSTRATE,
        "OuterIifeReturn",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&expr, PrimitiveName::Number);
}

/// A method on an `export default { … }` object is a served member
/// position of the `default` declaration: its body-derived return
/// evaluates through the whole-function producer (no body scan).
/// Mutation recipe: leaving default-export objects unmarked (the index
/// skips them) renders the method's return a semantic miss.
#[test]
fn flow_return_substrate_export_default_object_method_return_evaluates() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export default {
  m() {
    return 1;
  },
};
export type DefaultMethodReturn = ReturnType<(typeof import("/fixtures/flow_return_substrate.ts")["default"])["m"]>;
"#,
    );
    let (expr, _) = resolve_expr(
        &host,
        SUBSTRATE,
        "DefaultMethodReturn",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&expr, PrimitiveName::Number);
}

/// A mixed relation <-> flow component's published carriers self-root on
/// the UNION of every drained member's file roots across both domains:
/// editing the file of a NESTED flow member (a file no relation node
/// references) must invalidate the component — a stale warm candidate
/// would keep serving the pre-edit verdict. Mutation recipe: dropping the
/// flow member's file roots from the union (the pre-union behavior)
/// leaves the post-edit resolution stuck at "yes".
#[test]
fn flow_return_substrate_mixed_component_invalidates_on_nested_flow_member_edit() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        "/fixtures/mixed_b.ts",
        r#"
import type { RootBox } from "/fixtures/mixed_a";
export interface NextBox {
  next(): RootBox;
}
"#,
    );
    upsert_ts(
        &host,
        "/fixtures/mixed_c.ts",
        r#"
import type { NextBox } from "/fixtures/mixed_b";
export declare function makeBox(): NextBox;
export class Worker {
  run() {
    return makeBox();
  }
}
"#,
    );
    upsert_ts(
        &host,
        "/fixtures/mixed_a.ts",
        r#"
import { Worker } from "/fixtures/mixed_c";
export declare const worker: Worker;
export class RootBox {
  next() {
    return worker.run();
  }
}
"#,
    );
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
import type { RootBox } from "/fixtures/mixed_a";
import type { NextBox } from "/fixtures/mixed_b";
export type RootAssign = RootBox extends NextBox ? "yes" : "no";
"#,
    );
    let (expr, _) = resolve_substrate_alias(&host, "RootAssign");
    assert_string_literal(&expr, "yes");

    // Edit the nested flow member's file (mixed_c): `run` now returns a
    // number, so RootBox no longer extends NextBox. A stale candidate
    // rooted on the relation files only would keep serving "yes".
    upsert_ts(
        &host,
        "/fixtures/mixed_c.ts",
        r#"
export class Worker {
  run() {
    return 1;
  }
}
"#,
    );
    let (expr, _) = resolve_substrate_alias(&host, "RootAssign");
    assert_string_literal(&expr, "no");
}

/// A three-member recursive component discharges order-independently:
/// `a -> b -> c -> a` where only `a` carries a concrete seed. `c`'s
/// hold-only outcome upgrades once `a` admits; `b` upgrades on `c`'s
/// upgrade. Mutation recipe: dropping the empty-cycle callee from the
/// caller's holds (or a single drain-order pass) poisons the component.
#[test]
fn flow_return_substrate_three_member_cycle_discharges_on_the_seed() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function cycA(c: boolean) {
  if (c) return 1;
  return cycB(c);
}
export function cycB(c: boolean) {
  return cycC(c);
}
export function cycC(c: boolean) {
  return cycA(c);
}
export type CycChain = ReturnType<typeof cycA>;
"#,
    );
    let (expr, _) = resolve_substrate_alias(&host, "CycChain");
    assert_primitive(&expr, PrimitiveName::Number);
}

/// A forward-dependency cycle upgrades even when the drain reaches the
/// hold-only member before its target's Complete is admitted:
/// `a -> b`, `b -> 1 | c`, `c -> b | a`. Mutation recipe: admitting
/// Completes only when the drain reaches them poisons the component.
#[test]
fn flow_return_substrate_forward_dependency_cycle_is_order_independent() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export function fwdA(c: boolean) {
  return fwdB(c);
}
export function fwdB(c: boolean) {
  if (c) return 1;
  return fwdC(c);
}
export function fwdC(c: boolean) {
  if (c) return fwdB(c);
  return fwdA(c);
}
export type FwdChain = ReturnType<typeof fwdA>;
"#,
    );
    let (expr, _) = resolve_substrate_alias(&host, "FwdChain");
    assert_primitive(&expr, PrimitiveName::Number);
}

/// A class-field function initializer (`f = () => { … }`) is a served
/// class-member position: its body-derived return evaluates through the
/// whole-function producer (`number`, never a miss). Mutation
/// recipe: leaving the property path unmarked degrades this to a semantic
/// miss.
#[test]
fn flow_return_substrate_class_field_function_initializer_evaluates() {
    let host = make_host_with_footprint();
    upsert_ts(
        &host,
        SUBSTRATE,
        r#"
export class FieldHost {
  f = () => {
    return 1;
  };
}
export type FieldFnReturn = ReturnType<FieldHost["f"]>;
"#,
    );
    let (expr, _) = resolve_substrate_alias(&host, "FieldFnReturn");
    assert_primitive(&expr, PrimitiveName::Number);
}

/// The multi-seed component's fixed point: the two members' literal
/// seeds, unwidened. Literal widening at a return join is tsc's
/// SINGLE-contributor rule, and each member here joins its own seed with
/// the other member's discharged return — two contributors, so both
/// literals stay pinned (`"a" | 1`).
fn assert_union_is_the_two_literal_seeds(expr: &TypeExpr) {
    let TypeExpr::Union(members) = expr else {
        panic!("expected the `\"a\" | 1` union, got {expr:?}");
    };
    let mut has_string = false;
    let mut has_number = false;
    for member in members.iter() {
        match member {
            TypeExpr::Literal(LiteralValue::String(value)) if value == "a" => has_string = true,
            TypeExpr::Literal(LiteralValue::Number(value)) if *value == 1.0 => has_number = true,
            other => panic!("unexpected union arm: {other:?}"),
        }
    }
    assert!(has_string && has_number, "expected `\"a\" | 1`: {expr:?}");
}

const MULTI_SEED_CYCLE: &str = r#"
export function msa(c: boolean) {
  if (c) return "a";
  return msb(c);
}
export function msb(c: boolean) {
  if (c) return 1;
  return msa(c);
}
export type MsAFirst = ReturnType<typeof msa>;
export type MsBFirst = ReturnType<typeof msb>;
"#;

/// A multi-seed recursive component publishes the EQUATION FIXED POINT for
/// every member: `msa = "a" | msb`, `msb = 1 | msa` — both members admit
/// `"a" | 1`. This row drains the component A-FIRST; its
/// `…_msb_first` sibling drains the SAME source B-FIRST on its OWN host.
///
/// The two orders REQUIRE two hosts. `SemanticGraphStore` is host-owned and
/// outlives any one `ProjectSemanticDispatch`, so demanding both aliases
/// against a single host never executes the second drain — the first
/// demand closes the SCC and publishes every member, and the "reversed"
/// demand is served from the first order's state. A single-host pair
/// asserts one order twice and cannot fail in the direction it names.
///
/// Mutation recipe: revisiting only EmptyCycle members (never
/// Complete-with-holds) publishes `number` for the non-root member.
#[test]
fn flow_return_substrate_multi_seed_cycle_publishes_the_union_msa_first() {
    let host = make_host_with_footprint();
    upsert_ts(&host, SUBSTRATE, MULTI_SEED_CYCLE);
    let (expr_a, _) = resolve_substrate_alias(&host, "MsAFirst");
    assert_union_is_the_two_literal_seeds(&expr_a);
    let (expr_b, _) = resolve_substrate_alias(&host, "MsBFirst");
    assert_union_is_the_two_literal_seeds(&expr_b);
}

/// The B-FIRST drain of [`MULTI_SEED_CYCLE`] on a FRESH host — the order
/// the single-host pair never executed. Both members still admit `"a" | 1`:
/// the fixed point is the component's, not the entry point's.
#[test]
fn flow_return_substrate_multi_seed_cycle_publishes_the_union_msb_first() {
    let host = make_host_with_footprint();
    upsert_ts(&host, SUBSTRATE, MULTI_SEED_CYCLE);
    let (expr_b, _) = resolve_substrate_alias(&host, "MsBFirst");
    assert_union_is_the_two_literal_seeds(&expr_b);
    let (expr_a, _) = resolve_substrate_alias(&host, "MsAFirst");
    assert_union_is_the_two_literal_seeds(&expr_a);
}

const MULTI_SEED_CYCLE_UNDER_RELATION: &str = r#"
export interface NumberBox {
  next(): number;
}
export declare function makeNumberBox(): NumberBox;
export function msa(c: boolean) {
  if (c) return "a";
  return msb(c);
}
export function msb(c: boolean) {
  if (c) return 1;
  return msa(c);
}
export class PairHost {
  next() {
    return msb(true);
  }
}
export type PairAssign = PairHost extends NumberBox ? "yes" : "no";
export type MsAFirst = ReturnType<typeof msa>;
"#;

/// The same fixed point holds when the recursive flow component drains
/// under a RELATION root: `PairHost.next` calls `msb`, whose fixed-point
/// return is `"a" | 1` — NOT assignable to `number`. This row drains the
/// component COLD, from the relation root itself. Mutation recipe: the
/// under-approximation (`msb = number` at pop) admits "yes".
#[test]
fn flow_return_substrate_multi_seed_cycle_under_a_relation_root() {
    let host = make_host_with_footprint();
    upsert_ts(&host, SUBSTRATE, MULTI_SEED_CYCLE_UNDER_RELATION);
    let (expr, _) = resolve_substrate_alias(&host, "PairAssign");
    assert_string_literal(&expr, "no");
}

/// The relation root's answer is the same when the flow component was
/// already closed by an `msa`-rooted drain on this host — the other order
/// of the same two-demand sequence, on its OWN host. A relation that reads
/// a PUBLISHED component member must see the identical fixed point it
/// would have computed cold.
#[test]
fn flow_return_substrate_multi_seed_cycle_under_a_relation_root_after_a_flow_drain() {
    let host = make_host_with_footprint();
    upsert_ts(&host, SUBSTRATE, MULTI_SEED_CYCLE_UNDER_RELATION);
    let (pre, _) = resolve_substrate_alias(&host, "MsAFirst");
    assert_union_is_the_two_literal_seeds(&pre);
    let (expr, _) = resolve_substrate_alias(&host, "PairAssign");
    assert_string_literal(&expr, "no");
}
