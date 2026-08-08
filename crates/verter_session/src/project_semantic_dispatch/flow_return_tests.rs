//! @ai-generated - Direct-dispatch `FlowReturn` executor tests: symbolic
//! call carriers, the `this`-call fallback, loop transparency, degraded
//! shapes, recursion discharge (base-plus-recursion admits; the empty
//! cycle is ReturnOnly and never `never`), primitive widening, key
//! identity (overload ordinals, value-env exclusion), and family warm hits.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnKey, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;

const FLOW_EXEC_FIXTURE: &str = r#"
export declare function subLog(value: number): void;

export function subCallee(): { ok: string } {
  return { ok: "yes" };
}

export function subCallReturn() {
  return subCallee();
}

export function subCallAfterLoop() {
  for (let i = 0; i < 3; i++) subLog(i);
  return subCallee();
}

export class SubThisCall {
  helper(): number {
    return 1;
  }
  run() {
    return this.helper();
  }
}

export function subLoopReturn(n: number) {
  while (n > 0) {
    return n;
  }
  return 0;
}

export function subSwitchReturn(value: number) {
  switch (value) {
    case 1:
      return "a";
    default:
      return "b";
  }
}

export function subTryReturn() {
  try {
    return "a";
  } finally {
  }
}

export function subBaseRecursion(n: number) {
  if (n <= 0) return 0;
  return subBaseRecursion(n - 1);
}

export function subEmptyRecursion() {
  return subEmptyRecursion();
}

export function subLiteral() {
  return 1;
}

export function subParam(a: number) {
  return a;
}

export function subLocalConst() {
  const x = 1;
  return x;
}

export function subOverloaded(a: string): void;
export function subOverloaded(a: number) {
  return a;
}

/** Documented.
 * @returns {string} the documented payload
 */
export function subJsdocReturn() {
  return "doc";
}

export function subNonCallableCall() {
  const notFn = { a: 1 };
  return notFn();
}

export function subObservesBrokenInit() {
  const x = subSwitchReturn(1);
  return x;
}

export function subIgnoresBrokenInit() {
  const x = subSwitchReturn(1);
  return 2;
}

export function subInner() {
  return { pay: "load" };
}

export function subOuterCallsInner() {
  return subInner();
}

export declare const sideMarker: { tag: string };

export function subMemberWiden() {
  const other = sideMarker;
  const b = 1;
  return { a: other, b };
}

export function subMemberConstAssert() {
  const b = 1 as const;
  return { a: "s", b };
}

export function subMixedBareValue(flag: boolean) {
  if (flag) return 1;
  return;
}

export function subMemberBareMix(flag: boolean) {
  if (flag) return { b: 1 };
  return;
}

export function subShadowedOuterInit(notFn: number) {
  const x = notFn();
  {
    const x = 1;
    return x;
  }
}

export function subUnappliedParamWrite(x: string | number) {
  return { a: (x = "s"), b: x };
}

export function subWriteToUnreadSlot(x: number) {
  let dead = 0;
  dead = x;
  return 1;
}

export function subStandalonePrecedingWrite(x: string | number) {
  x = "s";
  return x;
}

export declare const q: number;

export function subBlockLetVsOuterConst() {
  {
    let q = 0;
    q = 1;
  }
  return q;
}

export function subHoistedVarBlock() {
  {
    var y = 1;
  }
  return y;
}

export function subBranchVarAny(flag: boolean) {
  if (flag) var x: any = 1;
  else var x: any = "s";
  return x;
}

export function subBranchVarOneArm(flag: boolean) {
  if (flag) {
    var w = 1;
  }
  return w;
}

export function subNestedBlockVar() {
  {
    {
      var y = 1;
    }
  }
  return y;
}

export function subEmptyIfThenVar(flag: boolean) {
  if (flag) {
  }
  var y = 1;
  return y;
}

export function subRedeclaredVar() {
  var y = 1;
  var y = 2;
  return y;
}

export function subVarShadowedByBlockLet() {
  var y = 1;
  {
    let y = "s";
  }
  return y;
}

export function subVarAnnotatedNoInit() {
  {
    var y: number | undefined;
  }
  return y;
}

export function subLetAnnotatedNoInit() {
  let y: number | undefined;
  return y;
}

export function subAnnotatedConstLiteral() {
  const y: string = "s";
  return y;
}

export function subAnnotatedConstLiteralPinned() {
  const y: "s" = "s";
  return y;
}

export function subRedeclareParamVar(x: string | number) {
  var x: string | number = "s";
  return x;
}

export function subRedeclareParamVarNested(x: string | number) {
  {
    var x: string | number = "s";
  }
  return x;
}

export function subForwardVarOverParam(x: string) {
  return x;
  var x = "s";
}

export function subLoopBodyVar(flag: boolean) {
  while (flag) {
    var v = 1;
  }
  return v;
}

export function subLoopBodyLet(flag: boolean) {
  while (flag) {
    let v = 1;
  }
  return 1;
}
"#;

const CANONICAL: &str = "/ws/flow-exec.ts";

fn make_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(FLOW_EXEC_FIXTURE),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
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

fn flow_result_for_file(
    dispatch: &ProjectSemanticDispatch<'_>,
    host: &VerterHost,
    canonical: &str,
    name: &str,
) -> (verter_type_expr::TypeExpr, bool) {
    let key = FlowReturnKey {
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
    };
    flow_result(dispatch, host, key)
}

fn flow_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    part: FunctionPartIdentity,
    overload_ordinal: u32,
) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            part,
            overload_ordinal,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    }
}

fn execute_flow(
    dispatch: &ProjectSemanticDispatch<'_>,
    key: FlowReturnKey,
) -> QueryResult<SemanticQueryOutput<SemanticQueryValue>> {
    dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
}

fn flow_result(
    dispatch: &ProjectSemanticDispatch<'_>,
    host: &VerterHost,
    key: FlowReturnKey,
) -> (verter_type_expr::TypeExpr, bool) {
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = execute_flow(dispatch, key)
    else {
        panic!("FlowReturn must produce a complete result");
    };
    let expr = host
        .project_node_to_type_expr_for_test(result.return_type())
        .expect("return node must project to TypeExpr");
    (expr, result.can_fall_through)
}

/// The POSITIONAL fail-closed assertion: the demanded return is a
/// DEGRADED SUCCESS whose value is the typed unmodelled-position MARKER —
/// never the wrong binding's value, never warm.
///
/// A whole-return position that the substrate cannot model is a POSITION
/// like any other; it is only "the whole frame" because there is nothing
/// else in the return. The load-bearing half is the same in either
/// spelling: whatever value the mis-binding WOULD have published is
/// absent, and nothing admits.
#[track_caller]
fn assert_whole_return_is_unmodeled_marker(
    dispatch: &ProjectSemanticDispatch<'_>,
    identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
    canonical: &str,
    what: &str,
) {
    match dispatch.execute_function_return_source(
        &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
        canonical,
    ) {
        super::flow_return::FunctionReturnNode::Flow(result) => {
            assert!(
                matches!(
                    dispatch.graph().node_data(result.return_type()).as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::UnmodeledPosition))
                ),
                "{what}: the position carries the typed marker, got {:?}",
                dispatch.graph().node_data(result.return_type())
            );
            assert_eq!(
                result.degradation(),
                Some(crate::semantic_query::FlowReturnDegradation::UnmodeledPosition),
                "{what}: the positional degradation reason"
            );
        }
        other => panic!("{what}: a degraded positional success, got {other:?}"),
    }
    assert_eq!(
        dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                dispatch.flow_return_key_for(identity)
            ))),
        0,
        "{what}: a degraded success is ReturnOnly — nothing warms"
    );
}

fn flow_is_miss(dispatch: &ProjectSemanticDispatch<'_>, key: FlowReturnKey) -> bool {
    matches!(
        execute_flow(dispatch, key),
        QueryResult::Error(QueryError::Miss)
    )
}

pub(crate) fn object_prop<'a>(
    expr: &'a verter_type_expr::TypeExpr,
    name: &str,
) -> &'a verter_type_expr::TypeExpr {
    let verter_type_expr::TypeExpr::Object(object) = expr else {
        panic!("expected object type, got {expr:?}");
    };
    object
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) if prop.string_name() == Some(name) => {
                Some(&prop.ty)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected prop {name} in {expr:?}"))
}

#[test]
fn flow_return_symbolic_call_resolves_complete() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, fallthrough) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subCallReturn",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert_eq!(
            object_prop(&expr, "ok"),
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
        assert!(!fallthrough);
    });
}

/// `return this.helper()` FAILS CLOSED — it does not publish `any`.
///
/// `this` is not a modeled receiver, so the call reaches none of the
/// content half's structural call arms and the shared shallow pass
/// answers the whole expression with a bare `any`. That `any` carries no
/// `ReturnType<callee>` carrier, so the leaf carrier gate never saw it
/// and it published clean and WARM — a fabricated value at a call
/// position, under a promise that a call with no structural arm fails
/// closed. The call-position verdict is taken on the expression FORM, so
/// the promise holds here.
///
/// The DISPOSITION is positional: the whole return of this member IS the
/// call, so the marker is the whole-return value and the result is a
/// degraded success admitting nothing — not a fabricated `any`, and not
/// a discarded composite (there is no sibling to keep at this position).
/// The composite-position TWIN — the same unmodelled call as ONE member
/// of an object literal — is in `flow_return_positional_tests`.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, for the record — the answer the
/// fail-closed arm declines to produce): `number`.
#[test]
fn flow_return_this_call_fails_closed() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "SubThisCall",
            FunctionPartIdentity::Member {
                member_path: Arc::from(vec![1u32].into_boxed_slice()),
            },
            0,
        );
        super::flow_return_lexical_tests::assert_flow_fails_closed(
            dispatch,
            "SubThisCall",
            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "SubThisCall must admit nothing"
        );
    });
}

#[test]
fn flow_return_return_free_loop_stays_transparent() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subCallAfterLoop",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert_eq!(
            object_prop(&expr, "ok"),
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
    });
}

#[test]
fn flow_return_return_bearing_loop_switch_try_are_degraded() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        for name in ["subLoopReturn", "subSwitchReturn", "subTryReturn"] {
            assert!(
                flow_is_miss(
                    dispatch,
                    flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody, 0)
                ),
                "{name} must stay degraded"
            );
            // Nothing admitted: a repeat demand runs cold again.
            assert!(
                flow_is_miss(
                    dispatch,
                    flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody, 0)
                ),
                "{name} must not admit a warm entry"
            );
        }
    });
}

#[test]
fn flow_return_base_plus_recursion_admits_widened_number() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, fallthrough) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subBaseRecursion",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        assert!(!fallthrough);
    });
}

#[test]
fn flow_return_empty_cycle_is_return_only_and_never_never() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subEmptyRecursion",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert!(
            flow_is_miss(dispatch, key.clone()),
            "an empty recursive cycle is ReturnOnly"
        );
        // Never admitted: the warm read misses, and nothing in the result is
        // `never`.
        assert!(
            dispatch
                .graph()
                .get_flow_return_result(dispatch.ctx, &key)
                .is_none(),
            "an empty recursive cycle admits no family value"
        );
        assert!(flow_is_miss(dispatch, key), "still cold on the next demand");
    });
}

#[test]
fn flow_return_literal_widens_at_return_position() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subLiteral",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        // The return-argument position widens a fresh top-level literal to
        // its base primitive (the flow IR's declaration lowering encodes the
        // positional rule); a `const`-asserted or arrow-body literal keeps
        // its literal node.
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

#[test]
fn flow_return_parameter_reference_substitutes_its_annotation() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subParam",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

#[test]
fn flow_return_local_const_reaching_definition() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subLocalConst",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        // A WIDENING-literal `const` binding widens at the return join —
        // TS7 oracle (`tsgo 7.0.0-dev.20260526.1 --declaration`):
        // `function f(){ const x = 1; return x; }` declares `(): number`.
        // (`1 as const` / an annotated `const x: 1` stay pinned — see
        // `flow_return_member_demand_preserves_const_asserted_local`.)
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

#[test]
fn flow_return_distinct_overload_ordinals_are_distinct_keys() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let bodiless = flow_key(
            dispatch,
            "subOverloaded",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let implementation = flow_key(
            dispatch,
            "subOverloaded",
            FunctionPartIdentity::DeclarationBody,
            1,
        );
        assert_ne!(
            bodiless, implementation,
            "distinct overload ordinals produce distinct keys"
        );
        // The bodiless overload has no served body — degraded, never confused
        // with the implementation's admitted result.
        assert!(flow_is_miss(dispatch, bodiless));
        let (expr, _) = flow_result(dispatch, &host, implementation);
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

#[test]
fn flow_return_key_carries_no_value_environment() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        // Two demands of the same function under the same context produce the
        // identical key (value bindings never enter it) — the second is a warm
        // family hit.
        let key = flow_key(
            dispatch,
            "subCallReturn",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let first = execute_flow(dispatch, key.clone());
        assert!(matches!(first, QueryResult::Value(_)));
        // The warm read runs against a FRESH view (the artifact the cold
        // build materialized is now visible).
        let warm_key = key.clone();
        with_dispatch(&host, |fresh| {
            let warm = fresh.graph().get_flow_return_result(fresh.ctx, &warm_key);
            assert!(
                warm.is_some(),
                "the second demand of the same function identity is a warm family hit"
            );
        });
        // The substitution axis is TYPE-ONLY: an empty substitution is the
        // only context the substrate constructs; a type-param binding changes
        // the key.
        let mut typed = key.clone();
        typed.context.type_substitution =
            crate::semantic_query::CanonicalTypeSubstitution::new(vec![(
                crate::semantic_query::SemanticNodeId(7),
                crate::semantic_query::SemanticNodeId(8),
            )]);
        assert_ne!(typed, key, "a type substitution fork is a distinct key");
    });
}

/// Key axes: `P R T L J`, the TYPE-ONLY substitution, the function slot
/// (name / part / overload ordinal), and the normalized type args are ALL
/// family identity. Two keys differing in any single axis are distinct
/// and never warm-hit each other. Mutation recipe: dropping any axis from
/// the key fails the distinctness half; failing the fresh-view family
/// lookup fails the isolation half.
#[test]
pub(crate) fn flow_return_keys_do_not_warm_hit_across_env_axes() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let base = flow_key(
            dispatch,
            "subCallReturn",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let with_env = |mutate: fn(&mut crate::semantic_query::FlowReturnContext)| {
            let mut key = base.clone();
            mutate(&mut key.context);
            key
        };
        for (axis, variant) in [
            ("P", with_env(|context| context.parse_env_hash = [0xAA; 16])),
            (
                "R",
                with_env(|context| context.resolve_env_hash = [0xBB; 16]),
            ),
            ("T", with_env(|context| context.type_env_hash = [0xCC; 16])),
            ("L", with_env(|context| context.lib_env_hash = [0xDD; 16])),
            (
                "J",
                with_env(|context| context.project_identity = [0xEE; 16]),
            ),
        ] {
            assert_ne!(
                variant, base,
                "a key differing only in the {axis} env axis must be distinct"
            );
        }
        let part_fork = flow_key(
            dispatch,
            "subCallReturn",
            FunctionPartIdentity::Initializer,
            0,
        );
        assert_ne!(part_fork, base, "the function part is key identity");
        let ordinal_fork = flow_key(
            dispatch,
            "subCallReturn",
            FunctionPartIdentity::DeclarationBody,
            1,
        );
        assert_ne!(ordinal_fork, base, "the overload ordinal is key identity");
        let mut args_fork = base.clone();
        args_fork.normalized_type_args =
            Arc::from(vec![crate::semantic_query::SemanticNodeId(9)].into_boxed_slice());
        assert_ne!(args_fork, base, "the normalized type args are key identity");

        // Warm isolation: compute the base, then prove a shifted-env key
        // does NOT warm-hit the base's family entry.
        let first = execute_flow(dispatch, base.clone());
        assert!(matches!(first, QueryResult::Value(_)));
        let shifted = with_env(|context| context.parse_env_hash = [0xAA; 16]);
        assert!(
            dispatch
                .graph()
                .get_flow_return_result(dispatch.ctx, &shifted)
                .is_none(),
            "a shifted-env key must not warm-hit the base entry"
        );
    });
}

// ---------------------------------------------------------------------------
// The sealed function-return consumer entry
// ---------------------------------------------------------------------------

fn return_identity(
    name: &str,
    part: FunctionPartIdentity,
    overload_ordinal: u32,
) -> verter_type_expr::facts::FlowFunctionReturnIdentity {
    verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(name),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: part,
        overload_ordinal,
    }
}

fn declared_return_slot(name: &str) -> verter_type_expr::locators::TypeBodySlot {
    verter_type_expr::locators::TypeBodySlot {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(name),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        path: Arc::from(
            vec![
                verter_type_expr::locators::TypeBodyPathStep::ValueSignature { ordinal: 0 },
                verter_type_expr::locators::TypeBodyPathStep::FunctionReturn,
            ]
            .into_boxed_slice(),
        ),
    }
}

/// The sealed helper's Flow arm constructs the IDENTICAL `FlowReturnKey` the
/// direct dispatch uses — the consumer-identity contract at the helper
/// boundary. Mutation recipe: deriving the slot or the context anywhere but
/// the one choke point forks the key and fails the equality half.
#[test]
fn function_return_helper_flow_arm_builds_the_identical_key() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let identity = return_identity("subCallReturn", FunctionPartIdentity::DeclarationBody, 0);
        let direct = flow_key(
            dispatch,
            "subCallReturn",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert_eq!(
            dispatch.flow_return_key_for(&identity),
            direct,
            "the sealed helper constructs the identical FlowReturnKey"
        );
        let source = verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone());
        let super::flow_return::FunctionReturnNode::Flow(result) =
            dispatch.execute_function_return_source(&source, CANONICAL)
        else {
            panic!("a Flow source is served by the FlowReturn producer");
        };
        let expr = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("return node must project to TypeExpr");
        assert_eq!(
            object_prop(&expr, "ok"),
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
        assert!(!result.can_fall_through);
        // The demand admitted under the helper-constructed key: a fresh view
        // warm-reads the same family entry.
        let key = dispatch.flow_return_key_for(&identity);
        with_dispatch(&host, |fresh| {
            assert!(
                fresh
                    .graph()
                    .get_flow_return_result(fresh.ctx, &key)
                    .is_some(),
                "the helper's demand admits under the identical key"
            );
        });
    });
}

/// The Declared arm lowers through the memoized locator rail — an authored
/// TS annotation and a JSDoc `@returns` recovery both deref their slot.
#[test]
fn function_return_helper_declared_arm_raises_authored_and_jsdoc() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let authored = verter_type_expr::facts::FunctionReturnSource::Declared(
            verter_type_expr::locators::FunctionReturnLocator::Authored(declared_return_slot(
                "subCallee",
            )),
        );
        let super::flow_return::FunctionReturnNode::Declared(hot) =
            dispatch.execute_function_return_source(&authored, CANONICAL)
        else {
            panic!("an authored return lowers through the locator rail");
        };
        let expr = host
            .project_node_to_type_expr_for_test(hot.node())
            .expect("declared return node must project to TypeExpr");
        assert_eq!(
            object_prop(&expr, "ok"),
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
        let jsdoc = verter_type_expr::facts::FunctionReturnSource::Declared(
            verter_type_expr::locators::FunctionReturnLocator::Jsdoc(declared_return_slot(
                "subJsdocReturn",
            )),
        );
        let super::flow_return::FunctionReturnNode::Declared(hot) =
            dispatch.execute_function_return_source(&jsdoc, CANONICAL)
        else {
            panic!("a JSDoc return lowers through the locator rail");
        };
        let expr = host
            .project_node_to_type_expr_for_test(hot.node())
            .expect("JSDoc return node must project to TypeExpr");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
    });
}

/// A degraded Flow evaluation surfaces the typed failure (never the
/// absent arm, never a fabricated node); `Absent` reports the absent
/// carrier.
#[test]
fn function_return_helper_degraded_and_absent_arms() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let loop_source = verter_type_expr::facts::FunctionReturnSource::Flow(return_identity(
            "subLoopReturn",
            FunctionPartIdentity::DeclarationBody,
            0,
        ));
        // The machinery root admits nothing; the TYPED failure rides the
        // transaction's root-failure channel to the caller (`Unresolved`
        // only when the cold build never ran).
        match dispatch.execute_function_return_source(&loop_source, CANONICAL) {
            super::flow_return::FunctionReturnNode::NoValue(
                crate::semantic_query::FlowReturnFailure::Unsupported(
                    crate::semantic_query::FlowReturnUnsupported::Loop,
                ),
            ) => {}
            other => panic!("a return-bearing loop degrades with the typed failure, got {other:?}"),
        }
        let absent = verter_type_expr::facts::FunctionReturnSource::Absent;
        assert!(matches!(
            dispatch.execute_function_return_source(&absent, CANONICAL),
            super::flow_return::FunctionReturnNode::Absent
        ));
    });
}

/// A mixed relation <-> flow component's batched member publish rides the
/// UNION carrier: the published flow member's family entry self-roots on
/// every component file — its own file AND the files of the relation
/// members' nodes. Mutation recipe: publishing the member with the root's
/// relation-only self-root set (the pre-union behavior) drops its own
/// file from the carrier.
#[test]
fn mixed_component_member_entry_self_roots_cover_all_component_files() {
    let host = make_host();
    for (canonical, source) in [
        (
            "/ws/mixed_b.ts",
            "import type { RootBox } from \"/ws/mixed_a\";\nexport interface NextBox {\n  next(): RootBox;\n}\n",
        ),
        (
            "/ws/mixed_c.ts",
            "import type { NextBox } from \"/ws/mixed_b\";\nexport declare function makeBox(): NextBox;\nexport class Worker {\n  run() {\n    return makeBox();\n  }\n}\n",
        ),
        (
            "/ws/mixed_a.ts",
            "import { Worker } from \"/ws/mixed_c\";\nexport declare const worker: Worker;\nexport class RootBox {\n  next() {\n    return worker.run();\n  }\n}\n",
        ),
    ] {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        });
    }
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/root.ts".to_string()),
        input_id: "/ws/root.ts".to_string(),
        source: Arc::from(
            "import type { RootBox } from \"/ws/mixed_a\";\nimport type { NextBox } from \"/ws/mixed_b\";\nexport type RootAssign = RootBox extends NextBox ? \"yes\" : \"no\";\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/root.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    let (expr, _) = host
        .evaluate_type_expression_with_audit(
            crate::typeinfo::types::EvaluateTypeExpressionRequest {
                scope: "/ws/root.ts".to_string(),
                expression: "RootAssign".to_string(),
                extra_imports: Vec::new(),
                mode: crate::semantic_query::ProjectionMode::Expanded,
                cacheable: false,
            },
        )
        .into_parts();
    let node = expr.ok().flatten().expect("RootAssign resolves");
    let projected = host
        .project_node_to_type_expr_for_test(node)
        .expect("RootAssign projects");
    assert_eq!(
        projected,
        verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(
            "yes".to_string()
        ))
    );
    with_dispatch(&host, |dispatch| {
        let key = FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                Arc::from("/ws/mixed_c.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("Worker"),
                FunctionPartIdentity::Member {
                    member_path: Arc::from(vec![0u32].into_boxed_slice()),
                },
                0,
            ),
            normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: dispatch.flow_return_context_for("/ws/mixed_c.ts"),
            demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
            input: crate::semantic_query::FlowInputContext::empty(),
        };
        let roots = dispatch
            .graph()
            .entry_self_root_canonicals_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
            .expect("the batched flow member's family entry is published");
        // The member's own file AND every relation member's node file
        // (the relation root's own demanding file is not a relation input
        // and is not part of the component's roots).
        for file in ["/ws/mixed_a.ts", "/ws/mixed_b.ts", "/ws/mixed_c.ts"] {
            assert!(
                roots.iter().any(|root| root.as_ref() == file),
                "the union carrier covers {file}: {roots:?}"
            );
        }
    });
}

/// Block-scoped bindings never escape their region: the `if` arm's
/// shadowing `const` does not leak past the arm — the outer `let`
/// binding's reaching definition answers the trailing `return x`.
/// Mutation recipe: a flat locals map across regions yields `string`
/// here.
#[test]
fn flow_return_block_scoped_shadowing_does_not_escape() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/shadow.ts".to_string()),
        input_id: "/ws/shadow.ts".to_string(),
        source: Arc::from(
            "export function shadow(c: boolean) {\n  let x = 1;\n  if (c) { const x = \"s\"; }\n  return x;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/shadow.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/shadow.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("shadow"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        let super::flow_return::FunctionReturnNode::Flow(result) = dispatch
            .execute_function_return_source(
                &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                "/ws/shadow.ts",
            )
        else {
            panic!("the shadowed return evaluates complete");
        };
        let expr = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("return node projects");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

/// A member-call self-recursion through the symbolic rail
/// (`return obj.f()` inside `obj.f`) propagates the nested demand's
/// degradation as a typed failure — the semantic-miss signature return is
/// NEVER counted as a contributor (a pre-fix evaluation published
/// `Complete` with the miss inside it). Mutation recipe: contributing the
/// miss-return node admits a `Complete` family value here.
#[test]
fn flow_return_member_call_self_recursion_is_return_only_not_complete_miss() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/member-recur.ts".to_string()),
        input_id: "/ws/member-recur.ts".to_string(),
        source: Arc::from("export const obj = {\n  f() {\n    return obj.f();\n  },\n};\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/member-recur.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/member-recur.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("obj"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::Member {
                member_path: Arc::from(vec![0u32].into_boxed_slice()),
            },
            overload_ordinal: 0,
        };
        assert_whole_return_is_unmodeled_marker(
            dispatch,
            &identity,
            "/ws/member-recur.ts",
            "the recursive member call",
        );
        // Nothing admitted: the family entry never materializes, and a
        // repeat demand runs cold.
        assert!(dispatch
            .graph()
            .get_flow_return_result(dispatch.ctx, &dispatch.flow_return_key_for(&identity))
            .is_none());
    });
}

/// A direct call to a callee with a DECLARED return serves the declared
/// carrier (never the narrowed body): `d(): string | number { return 1 }`
/// — the caller's contribution is `string | number`, not `number`.
/// Mutation recipe: always executing the callee's flow return narrows this
/// to `number`.
#[test]
fn flow_return_direct_call_prefers_the_callees_declared_return() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/declared-callee.ts".to_string()),
        input_id: "/ws/declared-callee.ts".to_string(),
        source: Arc::from(
            "export function d(): string | number {\n  return 1;\n}\nexport function c() {\n  return d();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/declared-callee.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/declared-callee.ts", "c");
        let verter_type_expr::TypeExpr::Union(arms) = &expr else {
            panic!("the declared union must survive, got {expr:?}");
        };
        assert_eq!(arms.len(), 2);
    });
}

/// A parameter that shadows the callee name makes the call an ordinary
/// parameter call — NEVER a self-recursive flow edge.
/// `function f(f: () => string) { return f() }` → `string` (not the empty
/// self-cycle's ReturnOnly). Mutation recipe: firing the DirectCall edge
/// on the shadowed name degrades this to a miss.
#[test]
fn flow_return_direct_call_respects_parameter_shadowing() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/shadowed-callee.ts".to_string()),
        input_id: "/ws/shadowed-callee.ts".to_string(),
        source: Arc::from("export function f(f: () => string) {\n  return f();\n}\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/shadowed-callee.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/shadowed-callee.ts", "f");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
    });
}

/// A namespace-local binding shadows the file-global one for a bare
/// callee inside the namespace: `N.g(){ return f() }` binds `N.f`
/// (string), never the global `f` (number). Mutation recipe: the
/// globally-highest overload ordinal binding flips this to `number`.
#[test]
fn flow_return_direct_call_prefers_the_namespace_local_binding() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/ns-callee.ts".to_string()),
        input_id: "/ws/ns-callee.ts".to_string(),
        source: Arc::from(
            "export function f() {\n  return 1;\n}\nnamespace N {\n  export function f() {\n    return \"s\";\n  }\n  export function g() {\n    return f();\n  }\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/ns-callee.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/ns-callee.ts", "N.g");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        );
    });
}

/// An unused declaration whose initializer cannot be modeled binds `any`
/// and never poisons the function (declarations are not return
/// contributions): `const unused = <unsupported>; return 2` stays
/// `number`. Mutation recipe: propagating the initializer's failure
/// degrades the whole function to a miss.
#[test]
fn flow_return_unused_binding_failure_binds_any_without_poisoning() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/unused-binding.ts".to_string()),
        input_id: "/ws/unused-binding.ts".to_string(),
        source: Arc::from(
            "export function broken(v: number) {\n  switch (v) {\n    default:\n      return v;\n  }\n}\nexport function survive() {\n  const unused = broken(1);\n  return 2;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/unused-binding.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/unused-binding.ts", "survive");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

/// A nested generic arrow keeps its own binder: `return <T>(x: T) => x`
/// composes a signature with one type parameter whose identity the
/// parameter type resolves to. Mutation recipe: dropping nested type
/// parameters interns the signature with an empty generic clause (and the
/// parameter as an unbound `T` reference).
#[test]
fn flow_return_nested_generic_arrow_keeps_its_type_parameters() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/nested-generic.ts".to_string()),
        input_id: "/ws/nested-generic.ts".to_string(),
        source: Arc::from("export function outer() {\n  return <T>(x: T) => x;\n}\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/nested-generic.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/nested-generic.ts", "outer");
        let verter_type_expr::TypeExpr::Function(function) = &expr else {
            panic!("the nested value is a function type, got {expr:?}");
        };
        assert_eq!(
            function.type_parameters.len(),
            1,
            "the nested generic keeps its binder: {expr:?}"
        );
        assert_eq!(function.type_parameters[0].name, "T");
        let verter_type_expr::TypeExpr::TypeParameter(param_ty) = &function.parameters[0].ty else {
            panic!(
                "the parameter resolves to the nested binder, got {:?}",
                function.parameters[0].ty
            );
        };
        assert_eq!(param_ty.name, "T");
    });
}

/// A long acyclic DirectCall chain charges the connected-demand ledger
/// one unit per inline frame: beyond the limit the chain degrades with a
/// typed budget failure instead of recursing unbounded. Mutation recipe:
/// dropping the inline-open charge lets the chain resolve.
#[test]
fn flow_return_direct_call_chain_charges_connected_work() {
    let host = make_host();
    let mut source = String::new();
    for index in 0..8 {
        source.push_str(&format!(
            "export function chain{index}() {{\n  return chain{}();\n}}\n",
            index + 1
        ));
    }
    source.push_str("export function chain8() {\n  return 1;\n}\n");
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/chain.ts".to_string()),
        input_id: "/ws/chain.ts".to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/chain.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        dispatch.set_connected_limits_for_tests(4, u16::MAX);
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/chain.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("chain0"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        match dispatch.execute_function_return_source(
            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
            "/ws/chain.ts",
        ) {
            super::flow_return::FunctionReturnNode::NoValue(
                crate::semantic_query::FlowReturnFailure::Budget(_),
            ) => {}
            other => panic!("the over-limit chain degrades with Budget, got {other:?}"),
        }
    });
}

/// A callee hold registered from inside a COMPOSITE is dropped on the
/// VALUE arm too, not only on the hold arm.
///
/// `settle_composite_part`'s own contract states the rule: "a hold is a
/// promise the SCC fixed point will union the hold TARGET's whole admitted
/// return into this entry's result. Inside a composite that promise is
/// false: the callee's return is not this object's value, it is one member
/// of it." The `Positional::Hold` arm truncated; the `Positional::Value`
/// arm did not — and the direct-call site registers a hold on the value
/// arm too whenever the callee popped as a PROVISIONAL member of the same
/// component. So the fixed point unioned the callee's whole return into
/// the composite anyway, exactly as the docstring says it must not.
///
/// `t3a` returns an object UNCONDITIONALLY, so a bare `1` arm is
/// impossible for any input: the union `1 | { m: 1 }` cannot be produced
/// by any execution of this program. (tsc types this pair `TS7023` —
/// implicit `any` from circular inference — so there is no checker answer
/// to disagree with; the falsifiable claim is the one the substrate makes
/// about its OWN composition, and `1` is not in `t3a`'s range.)
///
/// Mutation recipe: removing `self.holds.truncate(holds_before)` from the
/// `Positional::Value` arm restores `Union([Literal(1), Object{m}])`.
#[test]
fn a_composite_member_call_never_unions_the_callee_whole_return() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/composite-hold.ts".to_string()),
        input_id: "/ws/composite-hold.ts".to_string(),
        source: Arc::from(
            "export function t3a() {\n  return { m: t3b(true) };\n}\nexport function t3b(c: boolean) {\n  if (c) return t3a();\n  return 1;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/composite-hold.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/composite-hold.ts", "t3a");
        if let verter_type_expr::TypeExpr::Union(arms) = &expr {
            panic!(
                "`t3a` returns an object literal UNCONDITIONALLY — its return can never be a \
                 union with a bare callee arm; the composite leaked the member call's hold. \
                 Got {arms:?}"
            );
        }
    });
}

/// The sealed consumer entry classifies its fold by the OUTCOME, not by
/// the arm — so a budget edge never rides the FAITHFUL class, and the
/// runtime macro projection still faults on it.
///
/// The fold used to record ONE class for every non-`Clean` outcome. That
/// made a budget-truncated evaluation indistinguishable from "the surface
/// this substrate produced is faithful, only one position is marked", and
/// the runtime lane's containment then subtracted it: a budget trip
/// stopped faulting the `props: {...}` projection, which would publish a
/// runtime option object derived from an evaluation that never finished.
/// The budget edge is production-reachable through
/// `MAX_CONNECTED_PROJECTION_WORK`, not a test-only edge.
///
/// The TSC lane deliberately CONTAINS it — a budget-truncated class-member
/// inference leaves the authored declaration intact, and the file-level
/// aggregate still reports the precise budget class and warms nothing
/// (`tsc_class_inference_budget_is_exact_partial_and_non_cacheable`). So
/// the discriminating assertion is about the FAITHFUL class specifically,
/// not about containment in general.
///
/// Mutation recipe: mapping every `FlowReturnFailure` to
/// `FLOW_RETURN_UNINFERRED` (the pre-classification fold) fails both
/// assertions.
#[test]
fn a_budget_truncated_flow_return_folds_a_faulting_class_not_a_contained_one() {
    use crate::semantic_query::PartialReasonSet;

    let host = make_host();
    let mut source = String::new();
    for index in 0..8 {
        source.push_str(&format!(
            "export function chain{index}() {{\n  return chain{}();\n}}\n",
            index + 1
        ));
    }
    source.push_str("export function chain8() {\n  return 1;\n}\n");
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/budget-class.ts".to_string()),
        input_id: "/ws/budget-class.ts".to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/budget-class.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        dispatch.set_connected_limits_for_tests(4, u16::MAX);
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/budget-class.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("chain0"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        let scope = crate::request_context::ColdComputeCompletenessScope::enter();
        let node = dispatch.execute_function_return_source(
            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
            "/ws/budget-class.ts",
        );
        let observed = crate::request_context::current_cold_compute_completeness();
        scope.discard();

        assert!(
            matches!(
                node,
                super::flow_return::FunctionReturnNode::NoValue(
                    crate::semantic_query::FlowReturnFailure::Budget(_)
                )
            ),
            "the over-limit chain still degrades with Budget, got {node:?}"
        );
        let reasons = observed.reasons();
        assert!(
            !reasons.contains(PartialReasonSet::FLOW_RETURN_UNINFERRED),
            "a budget-truncated evaluation produced NO surface — it must never ride the \
             FAITHFUL class, which both macro lanes contain; got {reasons:?}"
        );
        assert!(
            !reasons.contains(PartialReasonSet::FLOW_RETURN_UNVERIFIED),
            "nor the degraded-success class whose member set is COMPLETE by definition — \
             the runtime lane contains that one, so riding it would let a no-surface \
             producer publish through any sibling contribution; got {reasons:?}"
        );
        assert!(
            reasons.contains(PartialReasonSet::FLOW_RETURN_NO_SURFACE),
            "it rides the TSC-only class instead, so the runtime `props` projection still \
             refuses while the authored TSC splice stays intact; got {reasons:?}"
        );
    });
}

/// ONE lexical binding authority: a hoisted nested function declaration
/// shadows the outer same-name callee, and its own return is beyond the
/// direct-call inventory — the call FAILS CLOSED (Unresolved), never
/// binding the outer `g(): number`. Mutation recipe: firing the index
/// DirectCall edge on the shadowed name admits `number` here.
#[test]
fn flow_return_direct_call_fails_closed_on_nested_function_declaration_shadow() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/fn-shadow.ts".to_string()),
        input_id: "/ws/fn-shadow.ts".to_string(),
        source: Arc::from(
            "export function g(): number {\n  return 1;\n}\nexport function f() {\n  function g() {\n    return \"x\";\n  }\n  return g();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/fn-shadow.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/fn-shadow.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("f"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        assert_whole_return_is_unmodeled_marker(
            dispatch,
            &identity,
            "/ws/fn-shadow.ts",
            "the shadowed direct call",
        );
    });
}

/// The same authority covers the self name: a nested `function f(): number`
/// inside `f` shadows BOTH the self-recursion hold and the declared nested
/// return — the call fails closed (Unresolved), never EmptyCycle, never
/// `number`. Mutation recipe: treating the call as a DirectSelfCall
/// degrades this with EmptyCycle instead.
#[test]
fn flow_return_self_call_shadowed_by_nested_function_declaration_fails_closed() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/self-fn-shadow.ts".to_string()),
        input_id: "/ws/self-fn-shadow.ts".to_string(),
        source: Arc::from(
            "export function f() {\n  function f(): number {\n    return 1;\n  }\n  return f();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/self-fn-shadow.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/self-fn-shadow.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("f"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        assert_whole_return_is_unmodeled_marker(
            dispatch,
            &identity,
            "/ws/self-fn-shadow.ts",
            "the shadowed self call",
        );
    });
}

/// A hoisted `var` is in scope from the function's first statement: a call
/// before its declarator binds the LOCAL (unbound at evaluation — `any`),
/// never the outer same-name callee's declared `number`. Mutation recipe:
/// dropping the hoisted-`var` scope seed admits `number` here.
#[test]
fn flow_return_forward_var_call_binds_the_unbound_local_not_the_outer_callee() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/forward-var.ts".to_string()),
        input_id: "/ws/forward-var.ts".to_string(),
        source: Arc::from(
            "export function g(): number {\n  return 1;\n}\nexport function f() {\n  return g();\n  var g = () => 1;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/forward-var.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/forward-var.ts", "f");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)
        );
    });
}

/// A region pre-declares its own lexical names: a call before a `const`
/// declarator binds the LOCAL (unbound — the TDZ-honest `any`), never the
/// outer same-name callee. Mutation recipe: dropping the region
/// pre-declare admits the outer `number` here.
#[test]
fn flow_return_forward_const_call_binds_the_unbound_local_not_the_outer_callee() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/forward-const.ts".to_string()),
        input_id: "/ws/forward-const.ts".to_string(),
        source: Arc::from(
            "export function g(): number {\n  return 1;\n}\nexport function f() {\n  return g();\n  const g = () => 1;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/forward-const.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/forward-const.ts", "f");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)
        );
    });
}

/// The pre-declare is ONE LEVEL deep: a `const` inside a nested block
/// stays block-local and never shadows the outer callee after the block —
/// the trailing `g()` still binds the outer `g(): number`. Mutation
/// recipe: pre-declaring across nested blocks flips this to `any`.
#[test]
fn flow_return_block_local_const_does_not_shadow_the_outer_callee() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/block-const.ts".to_string()),
        input_id: "/ws/block-const.ts".to_string(),
        source: Arc::from(
            "export function g(): number {\n  return 1;\n}\nexport function f(c: boolean) {\n  if (c) {\n    const g = () => \"s\";\n  }\n  return g();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/block-const.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/block-const.ts", "f");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

/// A root generic binder is in scope for the body's parameters and leaves:
/// `function id<T extends string>(x: T) { return x }` returns the BINDER
/// `T` (constraint `string`), never the file-scope alias `type T =
/// number`. Mutation recipe: lowering params without the binder env
/// resolves `T` to the alias and returns `number` here.
#[test]
fn flow_return_root_generic_binder_shadows_the_file_alias() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/root-binder.ts".to_string()),
        input_id: "/ws/root-binder.ts".to_string(),
        source: Arc::from(
            "export type T = number;\nexport function id<T extends string>(x: T) {\n  return x;\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/root-binder.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/root-binder.ts", "id");
        let verter_type_expr::TypeExpr::TypeParameter(param) = &expr else {
            panic!("the return is the root binder T, got {expr:?}");
        };
        assert_eq!(param.name, "T");
        assert_eq!(
            param.constraint.as_deref(),
            Some(&verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            )),
            "the binder keeps its `string` constraint: {expr:?}"
        );
    });
}

/// A return-free loop inside a NESTED function value stays transparent:
/// the nested body's control skeleton comes from the SAME single
/// inventory walk, never an empty skeleton. Mutation recipe: serving an
/// empty control skeleton for nested values degrades this to a miss.
#[test]
fn flow_return_nested_function_value_return_free_loop_stays_transparent() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/nested-loop.ts".to_string()),
        input_id: "/ws/nested-loop.ts".to_string(),
        source: Arc::from(
            "export function f() {\n  return (() => {\n    let x = 7;\n    for (let i = 0; i < 1; i++) {}\n    return x;\n  })();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/nested-loop.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/nested-loop.ts", "f");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

/// The lexical binding authority reaches INSIDE nested function values: a
/// hoisted nested declaration in the nested value's own body shadows the
/// outer same-name callee, and its return is beyond the direct-call
/// inventory — the call FAILS CLOSED (Unresolved), never binding the
/// outer `g(): number`. Mutation recipe: seeding the nested Lowerer with
/// an empty shadow set admits `number` here.
#[test]
fn flow_return_nested_value_hoisted_declaration_shadows_the_outer_callee() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/nested-fn-shadow.ts".to_string()),
        input_id: "/ws/nested-fn-shadow.ts".to_string(),
        source: Arc::from(
            "export function g(): number {\n  return 1;\n}\nexport function outer() {\n  return (() => {\n    function g() {\n      return \"x\";\n    }\n    return g();\n  })();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/nested-fn-shadow.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/nested-fn-shadow.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("outer"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        assert_whole_return_is_unmodeled_marker(
            dispatch,
            &identity,
            "/ws/nested-fn-shadow.ts",
            "the nested-value shadowed call",
        );
    });
}

/// The same authority covers a nested value's OWN name: an inner hoisted
/// declaration of the nested function's name shadows it — the call is
/// NEVER a DirectSelfCall (no self edge, no EmptyCycle); it fails closed
/// (Unresolved). Mutation recipe: an empty nested shadow set fires the
/// DirectSelfCall arm and degrades with EmptyCycle instead.
#[test]
fn flow_return_nested_value_self_name_shadowed_by_inner_declaration_fails_closed() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/nested-self-shadow.ts".to_string()),
        input_id: "/ws/nested-self-shadow.ts".to_string(),
        source: Arc::from(
            "export function outer() {\n  return (function helper() {\n    function helper(): number {\n      return 1;\n    }\n    return helper();\n  })();\n}\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/nested-self-shadow.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: Arc::from("/ws/nested-self-shadow.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from("outer"),
                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        };
        assert_whole_return_is_unmodeled_marker(
            dispatch,
            &identity,
            "/ws/nested-self-shadow.ts",
            "the nested-value self-name shadow",
        );
    });
}

// ────────────────────────────────────────────────────────────────────────
// Key axes (SCC-1), the split result model (SCC-2), the fourth budget
// layer, the sole warm rail (SCC-9), and the recorded materialised point.
// ────────────────────────────────────────────────────────────────────────

fn flow_result_value(
    dispatch: &ProjectSemanticDispatch<'_>,
    key: FlowReturnKey,
) -> crate::semantic_query::FlowReturnResult {
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = execute_flow(dispatch, key)
    else {
        panic!("expected a FlowReturn SUCCESS carrier");
    };
    (*result).clone()
}

fn key_hash(key: &FlowReturnKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// SCC-1 (`flow_return_key_covers_input_context_and_projection_demand`):
/// the key RETAINS the demand and input axes — two keys differing ONLY in
/// `ReturnProjectionDemand` (the walked projection path) or ONLY in
/// `FlowInputContext` (the contextual input identity) hash unequal and are
/// distinct identities (the family key embeds the full `FlowReturnKey`,
/// so distinct hashes ARE distinct candidate slots). The canonical
/// whole-return / empty-input point is one stable identity.
#[test]
pub(crate) fn flow_return_key_covers_input_context_and_projection_demand() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let base = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert!(base.demand.is_whole_return());
        assert!(base.input.is_empty());

        // Demand axis: same function/env/input, narrower projection path.
        let mut narrower = base.clone();
        narrower.demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("b")),
                        ),
                    ]);
                point
            },
        };
        assert_ne!(
            base, narrower,
            "the demand axis is identity, not decoration"
        );
        assert_ne!(
            key_hash(&base),
            key_hash(&narrower),
            "two keys differing only in the demand point must hash unequal"
        );

        // Input axis: same function/env/demand, a contextual input binding.
        let contextual =
            dispatch
                .graph()
                .intern_node(crate::semantic_query::SemanticNodeData::Primitive(
                    crate::semantic_query::PrimitiveKind::Number,
                ));
        let mut with_input = base.clone();
        with_input.input = crate::semantic_query::FlowInputContext {
            contextual_parameters: Arc::from(vec![contextual].into_boxed_slice()),
        };
        assert_ne!(
            base, with_input,
            "the input axis is identity, not decoration"
        );
        assert_ne!(
            key_hash(&base),
            key_hash(&with_input),
            "two keys differing only in the contextual input must hash unequal"
        );

        // The canonical point is stable: two independent constructions of
        // the whole-return / empty-input point are the SAME identity.
        let again = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert_eq!(base, again);
        assert_eq!(key_hash(&base), key_hash(&again));
    });
}

/// A narrower-than-whole-return demand point fails CLOSED with a typed
/// no-value outcome — never a silently widened whole-return result, never
/// a sibling materialisation the narrower demand did not ask for.
#[test]
fn flow_return_narrower_demand_point_fails_closed_unmodeled() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let mut key = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        key.demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("b")),
                        ),
                    ]);
                point
            },
        };
        assert!(
            flow_is_miss(dispatch, key.clone()),
            "an unmodeled demand point is a typed no-value outcome"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "an unmodeled demand point admits nothing"
        );
    });
}

/// SCC-2 — the split result/carrier model. A DEGRADED SUCCESS (a usable
/// value whose evaluation substituted a modeled-`any`) returns through
/// the SUCCESS carrier with its typed reason and admits NOTHING (memo,
/// facts, reverse index all untouched — candidate count stays zero); a
/// NO-VALUE failure returns through `Error(Miss)` and admits nothing; a
/// clean COMPLETE result admits warm. The two degraded shapes are
/// DIFFERENT public outcomes: one is a value, the other is not.
#[test]
fn flow_return_degraded_success_returns_value_and_admits_nothing() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        // Degraded SUCCESS: calling a non-callable binding.
        let degraded_key = flow_key(
            dispatch,
            "subNonCallableCall",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, degraded_key.clone());
        assert_eq!(
            result.degradation(),
            Some(crate::semantic_query::FlowReturnDegradation::NonCallableBinding),
            "a usable degraded value carries its typed reason on the SUCCESS carrier"
        );
        let projected = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("a degraded success is a USABLE value");
        assert_eq!(
            projected,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    degraded_key.clone()
                ))),
            0,
            "a degraded success is ReturnOnly: zero memo entries"
        );
        // A second demand recomputes (still a value, still not admitted).
        let again = flow_result_value(dispatch, degraded_key.clone());
        assert_eq!(again.degradation(), result.degradation());
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    degraded_key
                ))),
            0
        );

        // NO-VALUE failure: an unsupported `switch` body.
        let failure_key = flow_key(
            dispatch,
            "subSwitchReturn",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert!(
            flow_is_miss(dispatch, failure_key.clone()),
            "a no-value failure is Error(Miss), never a fabricated value"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    failure_key
                ))),
            0,
            "a no-value failure admits nothing"
        );

        // Clean COMPLETE: admits warm.
        let clean_key = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let clean = flow_result_value(dispatch, clean_key.clone());
        assert_eq!(clean.degradation(), None);
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(clean_key))),
            1,
            "a clean complete result is the warm-admissible arm"
        );
    });
}

/// Lexical binding identity (defect D): a read resolves to its
/// NEAREST-ENCLOSING-REGION binding, so an irrelevant same-named OUTER
/// declarator (whose initializer would degrade) never enters the slice —
/// the result is clean, correct, and warm-admissible. Before the fix the
/// name-keyed fan-out selected + lowered the outer `const x = notFn()`
/// declarator too, forcing a spurious `NonCallableBinding` degradation
/// (perpetual `ReturnOnly`) onto a clean program.
#[test]
fn flow_return_shadowed_outer_initializer_stays_clean_and_admits() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subShadowedOuterInit",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(
            result.degradation(),
            None,
            "the shadowed outer initializer is lexically unreachable from the \
             inner read — selecting/lowering it is the defect-D fan-out"
        );
        let projected = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("the clean result projects");
        assert_eq!(
            projected,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "a clean complete result admits warm — a spurious degradation would \
             hold it at ReturnOnly forever"
        );
    });
}

/// Unapplied write effects (defect B): the evaluator does not yet apply
/// `FlowSliceIR.effects` write retypes, so a slice carrying a whole-slot
/// write into a parameter (or any value-selected slot) MUST fail closed as
/// the `UnappliedWriteEffect` DEGRADED SUCCESS — a usable value, ReturnOnly,
/// never warm-admitted. Before the fix `subUnappliedParamWrite` published
/// `{ a: string, b: string | number }` as COMPLETE + warm-admissible while
/// tsc says `b: string` (the assignment narrows left-to-right) — wrong +
/// complete + non-degraded, the worst combination.
#[test]
fn flow_return_unapplied_write_effect_degrades_and_admits_nothing() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subUnappliedParamWrite",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(
            result.degradation(),
            Some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect),
            "a slice with an unapplied write effect into a value-selected \
             parameter slot must degrade (fail closed), never publish a \
             wrong type as complete"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    key.clone()
                ))),
            0,
            "the degraded success is ReturnOnly: zero memo entries"
        );
        // The value is still usable (degraded SUCCESS, not a no-value
        // failure).
        assert!(host
            .project_node_to_type_expr_for_test(result.return_type())
            .is_some());
    });
}

/// The B fail-closed rail is SLICE-scoped, not skeleton-scoped: a
/// whole-slot write into a binding the demanded path never reads
/// (`subWriteToUnreadSlot`: `dead = x` before `return 1`) stays outside
/// the lowered slice's effect obligations, so the result stays clean,
/// non-degraded, and warm-admissible. An over-broad rail scanning the
/// SKELETON's write summary (instead of the slice's lowered effects)
/// would spuriously degrade this — the D-class harm all over again.
#[test]
fn flow_return_write_to_unread_slot_stays_clean() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subWriteToUnreadSlot",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(
            result.degradation(),
            None,
            "a write outside the demanded slice cannot degrade the result"
        );
        let projected = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("the clean result projects");
        assert_eq!(
            projected,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "the clean result admits warm"
        );
    });
}

/// R1 — a STANDALONE preceding whole-slot write (`x = "s"; return x`)
/// reaches the lowered slice's effect obligations exactly like a write
/// embedded in the returned expression: the slot hub's reverse
/// eval-effect edge selects the write site, so the unapplied-write rail
/// degrades (fail closed, ReturnOnly). Before the fix the write site's
/// only edge ran INTO the hub, the effect frontier (out-edges only)
/// never selected it, `lower_slice_plan` silently dropped the write
/// (`effects=[]`), and `string | number` published as COMPLETE +
/// warm-admitted where tsc narrows to `string`.
#[test]
fn flow_return_standalone_preceding_write_degrades_and_admits_nothing() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subStandalonePrecedingWrite",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(
            result.degradation(),
            Some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect),
            "a preceding whole-slot parameter write is an unapplied effect: \
             evaluating past it would publish a type tsc narrows (oracle: `string`)"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    key.clone()
                ))),
            0,
            "the degraded success is ReturnOnly: zero memo entries"
        );
        // Still a USABLE degraded value, not a no-value failure.
        assert!(host
            .project_node_to_type_expr_for_test(result.return_type())
            .is_some());
    });
}

/// R4 companion (the R1 + R4 unit): a sibling-block `let q` with a
/// block-local write must NOT degrade a root-region `return q` that
/// reads the file-scope `declare const q: number` — block-scoped
/// bindings never hoist, so the all-same-name fallback may not
/// value-select the block hub (which R1's reverse effect edge would
/// then turn into a spurious `UnappliedWriteEffect`). Oracle: clean
/// `number`, warm-admissible.
#[test]
fn flow_return_block_let_write_stays_clean_for_root_read_of_outer_const() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subBlockLetVsOuterConst",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(
            result.degradation(),
            None,
            "the block-scoped `let q` is lexically unreachable from the root \
             read — a kind-blind hoisting fallback spuriously degrades this"
        );
        let projected = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("the clean result projects");
        assert_eq!(
            projected,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "the clean complete result admits warm"
        );
    });
}

/// R3 — a `var` inside a block is FUNCTION-scoped: the evaluator's
/// block restore must not clobber it (`{ var y = 1 } return y` is
/// `number` per tsc, not a silently-clean `any`). The content producer
/// already hoists the name; this pins the evaluator's kind-aware
/// scoping end-to-end (the graph-level fallback test only asserts edge
/// existence and cannot catch the evaluator wipe).
#[test]
fn flow_return_block_var_binding_hoists_to_the_function_scope() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subHoistedVarBlock",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(result.degradation(), None, "a hoisted var read is clean");
        let projected = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("the clean result projects");
        assert_eq!(
            projected,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            "the function-scoped `var` survives the block exit (oracle: `number`)"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "the clean complete result admits warm"
        );
    });
}

/// The `FailedBindingInitializer` degradation fires only when the failed
/// binding is OBSERVED: `return x` over a broken initializer is a
/// degraded success; ignoring the broken binding entirely stays a clean
/// complete result.
#[test]
fn flow_return_failed_binding_initializer_degrades_only_when_observed() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let observed = flow_result_value(
            dispatch,
            flow_key(
                dispatch,
                "subObservesBrokenInit",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert_eq!(
            observed.degradation(),
            Some(crate::semantic_query::FlowReturnDegradation::FailedBindingInitializer)
        );

        let ignored_key = flow_key(
            dispatch,
            "subIgnoresBrokenInit",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let ignored = flow_result_value(dispatch, ignored_key.clone());
        assert_eq!(
            ignored.degradation(),
            None,
            "an unobserved failed binding degrades nothing"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    ignored_key
                ))),
            1,
            "the clean sibling still admits warm"
        );
    });
}

/// The FOURTH budget non-admission layer: an over-budget demand slice is
/// refused at the `SemanticGraphStore` memo — the result is a typed
/// no-value Budget failure, ZERO memo candidates, ZERO slice-node
/// entries (the first three layers), and the SAME demand under the
/// restored armed budget completes and admits (the discrimination that
/// the budget, and nothing else, caused the refusal).
#[test]
pub(crate) fn flow_slice_budget_exceeded_is_return_only_at_the_memo() {
    use verter_semantic::analysis::flow::peeker::FlowSliceBudget;
    let host = make_host();
    host.project_type_store()
        .flow_slice()
        .set_budget_for_test(FlowSliceBudget {
            max_return_sites: 256,
            max_selected_nodes: 1,
        });
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subLocalConst",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        assert!(
            flow_is_miss(dispatch, key.clone()),
            "an over-budget slice is a typed Budget failure, never a value"
        );
        let store = host.project_type_store();
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                    key.clone()
                ))),
            0,
            "layer 4: the SemanticGraphStore memo admits nothing for an over-budget slice"
        );
        assert_eq!(
            store.flow_slice().hash_node().entry_count(),
            0,
            "the hash node publishes nothing (ReturnOnly)"
        );
        assert_eq!(
            store.flow_slice().lowered_node().entry_count(),
            0,
            "no slice hash exists, so the lowered store is unaddressable and empty"
        );

        // Restore the armed budget: the SAME key completes and admits —
        // the budget, and nothing else, caused the refusal.
        store
            .flow_slice()
            .set_budget_for_test(FlowSliceBudget::default());
        let result = flow_result_value(dispatch, key.clone());
        assert_eq!(result.degradation(), None);
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1
        );
    });
}

/// SCC-9 (sole warm rail, behavioral half): a warm `FlowReturn` read
/// validates through the `FlowBody` rooting + the unioned consumed facts
/// ONLY — it consults NO slice state. Discriminating probe: evict every
/// flow-slice artifact after the cold build; a fresh request's warm read
/// still serves (and rebuilds NO graph — a slice consult on the warm
/// path would have had to rebuild the evicted graph and republish the
/// evicted hash entry).
#[test]
fn flow_return_warm_read_consults_no_slice_state() {
    let host = make_host();
    let cold = with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        flow_result_value(dispatch, key)
    });
    let store = host.project_type_store();
    let builds_after_cold = store.flow_slice().graphs().build_count();
    assert!(builds_after_cold >= 1, "the cold path built the flow graph");

    // Drop EVERY flow-slice artifact for the canonical.
    store.flow_slice().remove_canonical(CANONICAL);
    assert_eq!(store.flow_slice().hash_node().entry_count(), 0);

    // A FRESH request (live store view) warm-reads the published entry.
    let warm = with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let warm_read = dispatch
            .graph()
            .get_flow_return_result(dispatch.ctx, &key)
            .expect("the published FlowReturn entry warm-validates on a live view");
        assert_eq!(warm_read, cold);
        flow_result_value(dispatch, key)
    });
    assert_eq!(warm, cold, "the warm read serves the published result");
    assert_eq!(
        store.flow_slice().graphs().build_count(),
        builds_after_cold,
        "the warm path re-derived NO slice state (no graph rebuild, no re-plan)"
    );
    assert_eq!(
        store.flow_slice().hash_node().entry_count(),
        0,
        "the warm path re-published NO slice artifact"
    );
}

/// §3.4 recorded-point identity: a published `FlowReturn` entry's
/// `satisfied_projection` is the point set the compute ACTUALLY produced
/// — the whole-return demand point — never an empty set and never a
/// synthetic slot preset detached from the key's own demand axis.
#[test]
fn flow_return_publishes_compute_recorded_whole_return_point() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subLiteral",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let _ = flow_result_value(dispatch, key.clone());
        let recorded = dispatch
            .graph()
            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                key.clone(),
            )))
            .expect("the clean complete result published");
        let points = recorded.points();
        assert_eq!(points.len(), 1, "one materialised point: the whole return");
        assert_eq!(
            points[0].point(),
            &key.demand.point,
            "the recorded point IS the whole-return demand point the compute served"
        );
        assert!(
            points[0].path().is_empty(),
            "the whole-return point materialises at the empty path"
        );

        // The MEMBER publish rail: a nested callee published at the SCC
        // drain carries ITS compute-recorded point (threaded through the
        // pending ledger — there is no publish-time default on the
        // member path).
        let root = flow_key(
            dispatch,
            "subOuterCallsInner",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        // The typed entry (the production consumer path) drains the
        // SCC-closed member batch onto the root's carrier.
        let step = dispatch.execute_flow_return(root);
        assert!(matches!(
            step,
            crate::semantic_query::FlowReturnStep::Complete(_)
        ));
        let member = flow_key(
            dispatch,
            "subInner",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        let member_recorded = dispatch
            .graph()
            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                member.clone(),
            )))
            .expect("the drained member published its own family entry");
        let member_points = member_recorded.points();
        assert_eq!(member_points.len(), 1);
        assert_eq!(
            member_points[0].point(),
            &member.demand.point,
            "the member entry's recorded point is the point ITS compute served"
        );
    });
}

// ---------------------------------------------------------------------------
// Member-projection demand (`ReturnType<typeof f>['b']` rail)
// ---------------------------------------------------------------------------

/// The single-named-member `ReturnProjectionDemand` point for one
/// declaration-body function.
fn member_flow_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    member: &str,
) -> FlowReturnKey {
    let mut key = flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody, 0);
    key.demand = crate::semantic_query::ReturnProjectionDemand {
        point: crate::semantic_query::demand::Demand::navigate(
            crate::semantic_query::demand::ProjectionPath::from_segments([
                crate::semantic_query::PathSegment::Member(
                    crate::semantic_query::PropertyKey::identifier(member),
                ),
            ]),
        ),
    };
    key
}

/// Whether any dispatched key under the capture names `needle` (the
/// sibling-materialisation detector for the member-demand tests).
fn capture_touches(snapshot: &crate::capture_token::CaptureSnapshot, needle: &str) -> bool {
    snapshot
        .dispatch_log
        .iter()
        .any(|entry| format!("{:?}", entry.key).contains(needle))
}

/// The member demand evaluates ONLY the demanded member: `b` (a
/// widening-literal local read) projects the widened `number`, and the
/// sibling binding's value root (`sideMarker`) never enters a dispatch
/// key. The whole-return demand on the SAME function is the positive
/// control: it DOES materialise the sibling, so the detector is proven
/// able to see a sibling materialisation.
#[test]
fn flow_return_member_demand_projects_widened_member_and_skips_sibling_binding() {
    // Member demand: sibling stays cold.
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let guard = crate::capture_token::CaptureToken::start_for_query("flow_member_demand");
        let (expr, _) = flow_result(
            dispatch,
            &host,
            member_flow_key(dispatch, "subMemberWiden", "b"),
        );
        let snapshot = guard.end();
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            "the demanded member widens its literal local read to number"
        );
        assert!(
            !capture_touches(&snapshot, "sideMarker"),
            "the elided sibling's value root must never enter a dispatch key"
        );
    });

    // Positive control on a FRESH host: the whole-return demand
    // materialises the sibling — the detector is discriminating.
    let control = make_host();
    with_dispatch(&control, |dispatch| {
        let guard = crate::capture_token::CaptureToken::start_for_query("flow_member_control");
        let (expr, _) = flow_result(
            dispatch,
            &control,
            flow_key(
                dispatch,
                "subMemberWiden",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        let snapshot = guard.end();
        assert_eq!(
            object_prop(&expr, "b"),
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        assert!(
            capture_touches(&snapshot, "sideMarker"),
            "positive control: the WHOLE-return evaluation materialises the sibling, \
             so a member-demand leak would have been visible to the detector"
        );
    });
}

/// `as const` preservation through the member demand: a const-asserted
/// literal local read stays the pinned literal — TS7 oracle
/// (`tsgo 7.0.0-dev.20260526.1 --declaration`): `const x = 1 as const; return { x }`
/// declares `{ x: 1 }`.
#[test]
fn flow_return_member_demand_preserves_const_asserted_local() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            member_flow_key(dispatch, "subMemberConstAssert", "b"),
        );
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::Number(1.0))
        );
    });
}

/// A member demand over a NON-OBJECT return fails closed with a typed
/// no-value outcome — never a silently widened whole-return result.
#[test]
fn flow_return_member_demand_on_non_object_return_is_typed_miss() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        assert!(flow_is_miss(
            dispatch,
            member_flow_key(dispatch, "subLiteral", "b")
        ));
    });
}

/// A member demand for a key the returned object does not carry fails
/// closed — never a fabricated `undefined` member.
#[test]
fn flow_return_member_demand_missing_member_is_typed_miss() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        assert!(flow_is_miss(
            dispatch,
            member_flow_key(dispatch, "subMemberWiden", "missing")
        ));
    });
}

/// A member demand over a body with a bare return / fallthrough arm
/// fails closed (the `undefined` arm cannot fold into a member access).
#[test]
fn flow_return_member_demand_with_bare_return_arm_is_typed_miss() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        assert!(flow_is_miss(
            dispatch,
            member_flow_key(dispatch, "subMemberBareMix", "b")
        ));
    });
}

/// Bare-return-as-void keeps its OTHER half: a bare return ALONGSIDE a
/// value return contributes `undefined` (never `void`, never dropped).
/// The `undefined` arm is a CONTRIBUTOR, so the value return's fresh
/// literal is no longer alone and stays pinned — tsgo 7.0.0-dev.20260526.1 on
/// `subMixedBareValue` is `1 | undefined`, not `number | undefined`.
#[test]
fn flow_return_mixed_bare_and_value_returns_include_undefined_arm() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subMixedBareValue",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        let verter_type_expr::TypeExpr::Union(arms) = &expr else {
            panic!("expected `1 | undefined`, got {expr:?}");
        };
        assert!(arms.contains(&verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Undefined
        )));
        assert!(
            arms.contains(&verter_type_expr::TypeExpr::Literal(
                verter_type_expr::LiteralValue::Number(1.0)
            )),
            "the value return's literal stays pinned beside the \
             `undefined` arm: {expr:?}"
        );
    });
}

/// A deeply nested array literal that trips the shallow leaf-lowering
/// budget (`> MAX_SEMANTIC_INFERENCE_DEPTH` nesting levels).
fn budget_tripping_array() -> String {
    let levels = 70;
    let mut out = String::new();
    // bounded-loop: fixed 70-level fixture constructor.
    for _ in 0..levels {
        out.push('[');
    }
    out.push('0');
    // bounded-loop: fixed 70-level fixture constructor.
    for _ in 0..levels {
        out.push(']');
    }
    out
}

/// The demand slice is the ONLY lowered content: an UNREAD binding's
/// initializer is outside every selected slot, so its content never
/// lowers — a leaf-lowering budget edge inside it cannot exist, and the
/// whole-return evaluation stays complete. Mutation recipe: lowering the
/// whole body (a pre-slice whole-function evaluator) trips the leaf
/// budget on the unread initializer and degrades the function to a miss.
#[test]
fn flow_return_unread_binding_content_never_lowers() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/unread-content.ts".to_string()),
        input_id: "/ws/unread-content.ts".to_string(),
        source: Arc::from(format!(
            "export function sliced() {{\n  const unused = {};\n  return 2;\n}}\n",
            budget_tripping_array()
        )),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/unread-content.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let (expr, _) = flow_result_for_file(dispatch, &host, "/ws/unread-content.ts", "sliced");
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            "the unread binding's initializer stays unlowered; its budget edge cannot poison"
        );
    });
}

/// The member demand lowers ONLY the demanded member's content: an
/// elided sibling member whose value would trip the leaf-lowering
/// budget never lowers, so the demanded member still projects complete.
/// Mutation recipe: lowering every member of the returned object (a
/// pre-slice whole-function evaluator) trips the budget on the sibling
/// and degrades the member demand to a miss.
#[test]
fn flow_return_member_demand_never_lowers_elided_sibling_content() {
    let host = make_host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ws/member-sibling-content.ts".to_string()),
        input_id: "/ws/member-sibling-content.ts".to_string(),
        source: Arc::from(format!(
            "export function memberSliced() {{\n  return {{ a: {}, b: 1 }};\n}}\n",
            budget_tripping_array()
        )),
        file_language: crate::LanguageRegistry::global()
            .classify_static("/ws/member-sibling-content.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    with_dispatch(&host, |dispatch| {
        let mut key = FlowReturnKey {
            function: dispatch.flow_function_slot_for(
                Arc::from("/ws/member-sibling-content.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("memberSliced"),
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
            normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: dispatch.flow_return_context_for("/ws/member-sibling-content.ts"),
            demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
            input: crate::semantic_query::FlowInputContext::empty(),
        };
        key.demand = crate::semantic_query::ReturnProjectionDemand {
            point: crate::semantic_query::demand::Demand::navigate(
                crate::semantic_query::demand::ProjectionPath::from_segments([
                    crate::semantic_query::PathSegment::Member(
                        crate::semantic_query::PropertyKey::identifier("b"),
                    ),
                ]),
            ),
        };
        let (expr, _) = flow_result(dispatch, &host, key);
        assert_eq!(
            expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            "the elided sibling's content stays unlowered; its budget edge cannot poison"
        );
    });
}

/// The whole-return and single-member demand points COEXIST as distinct
/// family candidates: neither satisfies the other (the §3.4 two-gate
/// hit over the RECORDED materialised point), and a warm re-read of the
/// whole point after the member publish still serves the whole object.
#[test]
fn flow_return_member_and_whole_demands_coexist_as_candidates() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let (whole, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subMemberWiden",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert!(matches!(&whole, verter_type_expr::TypeExpr::Object(_)));
        let (member, _) = flow_result(
            dispatch,
            &host,
            member_flow_key(dispatch, "subMemberWiden", "b"),
        );
        assert_eq!(
            member,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        // Warm re-reads serve each point's own candidate.
        let (whole_again, _) = flow_result(
            dispatch,
            &host,
            flow_key(
                dispatch,
                "subMemberWiden",
                FunctionPartIdentity::DeclarationBody,
                0,
            ),
        );
        assert!(matches!(
            &whole_again,
            verter_type_expr::TypeExpr::Object(_)
        ));
        let (member_again, _) = flow_result(
            dispatch,
            &host,
            member_flow_key(dispatch, "subMemberWiden", "b"),
        );
        assert_eq!(
            member_again,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
    });
}

/// Registry-live guard (`GuardId::FlowReturnRoutesThroughProjectSemanticDispatch`)
/// — the `SemanticQueryKey::FlowReturn` arm dispatches through
/// `ProjectSemanticDispatch::execute → SemanticGraphStore`, and the stored
/// result IS the dispatched result: the same key read back from the graph
/// store serves the identical published `FlowReturnResult`, and the
/// consumer-facing function-return helper builds the IDENTICAL dispatch
/// key (`function_return_helper_flow_arm_builds_the_identical_key`), so
/// every consumer route funnels into this one dispatch. The
/// no-second-constructor half is held by review pending a structural
/// construction confinement on `FlowReturnResult` (its production
/// constructors all live on the `build_flow_return` compute path today).
#[test]
pub(crate) fn flow_return_routes_through_project_semantic_dispatch() {
    let host = make_host();
    let (key, dispatched) = with_dispatch(&host, |dispatch| {
        let key = flow_key(
            dispatch,
            "subLocalConst",
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        // Cold: the ONLY producing route is `execute` on the dispatch.
        let dispatched = flow_result_value(dispatch, key.clone());
        assert_eq!(dispatched.degradation(), None);
        (key, dispatched)
    });

    // Against a FRESH view (the cold build's artifact is now visible):
    // the graph store — the dispatch's sole publication surface — serves
    // the SAME result for the same key, and re-execution serves the
    // published candidate (no second producer path constructs a
    // divergent result).
    with_dispatch(&host, |fresh| {
        let stored = fresh
            .graph()
            .get_flow_return_result(fresh.ctx, &key)
            .expect("the dispatched FlowReturn published to the graph store");
        assert_eq!(
            stored, dispatched,
            "the stored candidate is the dispatched result — dispatch → \
             SemanticGraphStore is the one serving path"
        );
        let warm = flow_result_value(fresh, key.clone());
        assert_eq!(warm, dispatched);
    });
}

// ────────────────────────────────────────────────────────────────────────
// Function-scoped (`var`) reaching definitions: conditional-definition
// fail-closed, declarator annotations, parameter redeclaration, and the
// loop-body `var`.
// ────────────────────────────────────────────────────────────────────────

/// Assert one function's flow return is CLEAN (no degradation), projects
/// to `expected`, and admits exactly one warm candidate.
fn assert_clean_warm(
    host: &Arc<VerterHost>,
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    expected: &verter_type_expr::TypeExpr,
) {
    let key = flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody, 0);
    let result = flow_result_value(dispatch, key.clone());
    assert_eq!(result.degradation(), None, "{name} must stay clean");
    let projected = host
        .project_node_to_type_expr_for_test(result.return_type())
        .unwrap_or_else(|| panic!("{name} must project"));
    assert_eq!(&projected, expected, "{name} return type");
    assert_eq!(
        dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
        1,
        "{name} must admit warm"
    );
}

/// Assert one function's flow return is a DEGRADED success carrying
/// `reason` — a usable value, ReturnOnly, zero warm candidates.
fn assert_degraded_return_only(
    host: &Arc<VerterHost>,
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    reason: crate::semantic_query::FlowReturnDegradation,
) {
    let key = flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody, 0);
    let result = flow_result_value(dispatch, key.clone());
    assert_eq!(result.degradation(), Some(reason), "{name} must degrade");
    assert_eq!(
        dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
        0,
        "{name} degraded success is ReturnOnly: zero memo entries"
    );
    assert!(
        host.project_node_to_type_expr_for_test(result.return_type())
            .is_some(),
        "{name} degraded success still carries a usable value"
    );
}

/// A `var` whose reaching definition was recorded inside an `if` arm is
/// observed AFTER the arms rejoin, where the substrate has no branch-join
/// algebra over the function-scoped layer: the last-evaluated arm's value
/// would publish as the reaching definition. That is a wrong answer
/// published clean + warm (`if (flag) var x: any = 1; else var x: any =
/// "s"; return x` published `string` where the oracle is `any`), so the
/// observation fails closed as a degraded success instead.
///
/// The one-armed form is the accepted cost: `if (flag) { var w = 1 }
/// return w` returns a coincidentally-correct `number` today, and tsc
/// REJECTS that program outright (TS2454, used before assigned) — fail
/// closed is the contract.
#[test]
fn flow_return_conditionally_defined_var_degrades_at_observation() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        assert_degraded_return_only(
            &host,
            dispatch,
            "subBranchVarAny",
            crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
        );
        assert_degraded_return_only(
            &host,
            dispatch,
            "subBranchVarOneArm",
            crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
        );
    });
}

/// The conditional-definition rail is CONDITIONAL-DEPTH keyed, not
/// nesting keyed: a plain block executes unconditionally, so every one of
/// these `var` bindings keeps its reaching definition and stays clean +
/// warm. A blanket "any nested `var` degrades" rule breaks all of them.
#[test]
fn flow_return_unconditional_var_definitions_stay_clean_and_warm() {
    let host = make_host();
    let number = verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number);
    with_dispatch(&host, |dispatch| {
        // `{ var y = 1 } return y`
        assert_clean_warm(&host, dispatch, "subHoistedVarBlock", &number);
        // `{ { var y = 1 } } return y`
        assert_clean_warm(&host, dispatch, "subNestedBlockVar", &number);
        // `if (flag) { } var y = 1; return y`
        assert_clean_warm(&host, dispatch, "subEmptyIfThenVar", &number);
        // `var y = 1; var y = 2; return y`
        assert_clean_warm(&host, dispatch, "subRedeclaredVar", &number);
        // `var y = 1; { let y = "s"; } return y`
        assert_clean_warm(&host, dispatch, "subVarShadowedByBlockLet", &number);
    });
}

/// A declarator's authored TS annotation is the binding's DECLARED type.
/// An initializer-less declarator seeds from it (`var y: number |
/// undefined;` is `number | undefined`, not the unbound implicit `any`),
/// and an annotated `const` publishes the annotation instead of its
/// initializer's pinned literal (`const y: string = "s"` is `string`, not
/// `"s"`) — the annotation SUPPLIES the type rather than merely
/// suppressing widening. A literal annotation still pins (`const y: "s" =
/// "s"` stays `"s"`).
#[test]
fn flow_return_declarator_annotation_supplies_the_binding_type() {
    let host = make_host();
    let number_or_undefined = verter_type_expr::TypeExpr::union(vec![
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined),
    ]);
    with_dispatch(&host, |dispatch| {
        assert_clean_warm(
            &host,
            dispatch,
            "subVarAnnotatedNoInit",
            &number_or_undefined,
        );
        assert_clean_warm(
            &host,
            dispatch,
            "subLetAnnotatedNoInit",
            &number_or_undefined,
        );
        assert_clean_warm(
            &host,
            dispatch,
            "subAnnotatedConstLiteral",
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        );
        assert_clean_warm(
            &host,
            dispatch,
            "subAnnotatedConstLiteralPinned",
            &verter_type_expr::TypeExpr::Literal(verter_type_expr::LiteralValue::String(
                "s".to_string(),
            )),
        );
    });
}

/// A hoisted `var` that REDECLARES a parameter name rebinds that name for
/// the whole function: the declarator's reaching definition wins over the
/// parameter's declared type (`function f(x: string | number) { var x:
/// string | number = "s"; return x }` is `string`). A read the declarator
/// never reached still resolves to the PARAMETER, never a fabricated
/// `any`.
#[test]
fn flow_return_hoisted_var_redeclaring_a_parameter_wins_over_the_parameter() {
    let host = make_host();
    let string = verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    with_dispatch(&host, |dispatch| {
        assert_clean_warm(&host, dispatch, "subRedeclareParamVar", &string);
        assert_clean_warm(&host, dispatch, "subRedeclareParamVarNested", &string);
        // `return x; var x = "s"` — the declarator is unreachable, so the
        // read resolves to the parameter.
        assert_clean_warm(&host, dispatch, "subForwardVarOverParam", &string);
    });
}

/// A return-free loop is fall-through TRANSPARENT only while it binds
/// nothing that outlives it. A `var` declared in its body is
/// function-scoped and its reaching definition depends on the loop's
/// iteration count, which the substrate does not model: `while (flag) {
/// var v = 1 } return v` published a clean, warm `any` where the oracle is
/// `number`. It now fails closed through the existing typed loop rail,
/// exactly like `switch` / `try`. A loop binding only block-scoped names
/// stays transparent.
#[test]
fn flow_return_return_free_loop_declaring_a_var_fails_closed() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        assert!(
            flow_is_miss(
                dispatch,
                flow_key(
                    dispatch,
                    "subLoopBodyVar",
                    FunctionPartIdentity::DeclarationBody,
                    0
                )
            ),
            "a return-free loop declaring a `var` must fail closed"
        );
        assert_clean_warm(
            &host,
            dispatch,
            "subLoopBodyLet",
            &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        );
    });
}

// ────────────────────────────────────────────────────────────────────────
// Public-API SCC flight release
// ────────────────────────────────────────────────────────────────────────

const SCC_CANONICAL: &str = "/ws/flow-scc.ts";

/// Two mutual components. `scCleanA`/`scCleanB` close cleanly (both
/// members admit warm); `scDegradedA`/`scDegradedB` carry an unapplied
/// write effect, so the whole component is a degraded success —
/// `ReturnOnly`, never warm.
const SCC_FIXTURE: &str = r#"
export function scCleanA(c: boolean) {
  if (c) return 1;
  return scCleanB(c);
}

export function scCleanB(c: boolean) {
  if (c) return 2;
  return scCleanA(c);
}

export function scDegradedA(c: boolean) {
  if (c) return 1;
  return scDegradedB(c);
}

export function scDegradedB(c: boolean) {
  let z = 1;
  z = 2;
  return scDegradedA(!!z);
}

export function scResA(c: boolean) {
  if (c) return scResB(c);
  return 0;
}

export function scResB(c: boolean) {
  return scResA(c);
}

export function scRingX(c: boolean) {
  if (c) return scRingY(c);
  return 0;
}

export function scRingY(c: boolean) {
  return scRingZ(c);
}

export function scRingZ(c: boolean) {
  return scRingX(c);
}
"#;

fn make_scc_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(SCC_CANONICAL.to_string()),
        input_id: SCC_CANONICAL.to_string(),
        source: Arc::from(SCC_FIXTURE),
        file_language: crate::LanguageRegistry::global()
            .classify_static(SCC_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn scc_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(SCC_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(SCC_CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    }
}

/// One PUBLIC-API demand: a fresh top-level dispatch per call, exactly as
/// an external `SemanticQueryApi` consumer issues it.
fn scc_public_demand(
    host: &Arc<VerterHost>,
    name: &str,
) -> (
    QueryResult<SemanticQueryOutput<SemanticQueryValue>>,
    usize,
    Vec<SemanticQueryKey>,
) {
    with_dispatch(host, |dispatch| {
        let key = scc_key(dispatch, name);
        let result = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
        let candidates = dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
        let retained = dispatch.graph().retained_claimed_flight_keys_for_tests();
        (result, candidates, retained)
    })
}

#[track_caller]
fn scc_expect_value(
    host: &Arc<VerterHost>,
    name: &str,
) -> (
    crate::semantic_query::FlowReturnResult,
    usize,
    Vec<SemanticQueryKey>,
) {
    let (result, candidates, retained) = scc_public_demand(host, name);
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(payload),
        ..
    }) = result
    else {
        panic!("{name} must answer through the public API, got {result:?}");
    };
    ((*payload).clone(), candidates, retained)
}

/// The public `execute(FlowReturn)` entry MUST release the component's
/// drained member flights, in EITHER demand order.
///
/// The machinery root is the only place a flow SCC's deferred member
/// batch is published or retired. Before the fix only
/// `execute_flow_return_root` did that, and the public
/// `SemanticQueryApi::execute` reached the family cold build through the
/// generic dispatch instead — so after the first demand closed, every
/// non-root member of the component was left with a CLAIMED, uncompleted
/// in-flight entry whose owner registration had already dropped. The next
/// demand of that member joined the stale entry, `register_wait`
/// reported a cycle against an inactive owner, and the caller received a
/// PERMANENT false `QueryResult::Recursive` — an incorrect public result
/// AND persistent lifecycle poison.
///
/// Both orders run on SEPARATE FRESH HOSTS so neither leg can be carried
/// by the other's warm state.
#[test]
fn flow_return_public_execute_releases_drained_members_both_orders() {
    for (first, second) in [("scCleanA", "scCleanB"), ("scCleanB", "scCleanA")] {
        let host = make_scc_host();

        let (first_result, first_candidates, first_retained) = scc_expect_value(&host, first);
        assert_eq!(
            first_result.degradation(),
            None,
            "{first} closes its component cleanly"
        );
        assert_eq!(
            first_candidates, 1,
            "{first} is the machinery root and admits warm"
        );
        assert!(
            first_retained.is_empty(),
            "{first} must leave no claimed/uncompleted flight: {first_retained:?}"
        );

        // The DRAINED member, demanded through the public API on the same
        // host. A stale member flight surfaces here as a permanent false
        // `Recursive`.
        let (second_raw, second_candidates, second_retained) = scc_public_demand(&host, second);
        assert!(
            !matches!(second_raw, QueryResult::Recursive(_)),
            "{second} must never surface a false Recursive after {first} drained it"
        );
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(second_result),
            ..
        }) = second_raw
        else {
            panic!("{second} must answer through the public API");
        };
        assert_eq!(
            second_result.degradation(),
            None,
            "{second} closes its component cleanly"
        );
        assert_eq!(
            second_candidates, 1,
            "{second} was drained onto the root's carrier and reads warm"
        );
        assert!(
            second_retained.is_empty(),
            "{second} must leave no claimed/uncompleted flight: {second_retained:?}"
        );

        // Each member publishes ITS OWN return sites in source order
        // (own literal first, the component contribution second) — and
        // the SAME value whichever member was demanded first, which is
        // the order-independence half of the contract.
        for name in ["scCleanA", "scCleanB"] {
            let (result, _, _) = scc_expect_value(&host, name);
            let projected = host
                .project_node_to_type_expr_for_test(result.return_type())
                .expect("a component member projects");
            let verter_type_expr::TypeExpr::Union(arms) = &projected else {
                panic!("{name} publishes the component union, got {projected:?}");
            };
            // Every member of a mutual component shares the component's
            // fixed point, so both publish the SAME arm set in either
            // demand order. Arm ORDER follows the root's accumulation
            // order and is not part of this contract.
            let mut arms: Vec<String> = arms.iter().map(|arm| format!("{arm:?}")).collect();
            arms.sort();
            assert_eq!(
                arms,
                vec![
                    format!("{:?}", verter_type_expr::TypeExpr::number_literal(1.0)),
                    format!("{:?}", verter_type_expr::TypeExpr::number_literal(2.0)),
                ],
                "{name} publishes the component's exact fixed point (demand order {first} → {second})"
            );
        }
    }

    // The DEGRADED leg: the whole component is a degraded success, so the
    // batch is ABORTED and RETIRED rather than published. Nothing warms,
    // and no member flight is retained.
    for (first, second) in [
        ("scDegradedA", "scDegradedB"),
        ("scDegradedB", "scDegradedA"),
    ] {
        let host = make_scc_host();

        let (first_result, first_candidates, first_retained) = scc_expect_value(&host, first);
        assert!(
            first_result.degradation().is_some(),
            "{first} carries the component's typed degradation"
        );
        assert_eq!(
            first_candidates, 0,
            "{first} is a degraded success: ReturnOnly, zero memo entries"
        );
        assert!(
            first_retained.is_empty(),
            "{first} must retire every member flight: {first_retained:?}"
        );

        let (second_raw, second_candidates, second_retained) = scc_public_demand(&host, second);
        assert!(
            !matches!(second_raw, QueryResult::Recursive(_)),
            "{second} must never surface a false Recursive after {first} aborted the batch"
        );
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(second_result),
            ..
        }) = second_raw
        else {
            panic!("{second} must answer through the public API");
        };
        assert!(
            second_result.degradation().is_some(),
            "{second} carries the component's typed degradation"
        );
        assert_eq!(
            second_candidates, 0,
            "{second} never warms off an aborted batch: zero memo entries, zero backfill"
        );
        assert!(
            second_retained.is_empty(),
            "{second} must retire every member flight: {second_retained:?}"
        );
    }
}

/// A drained SCC member's flight must be RETIRED FROM THE TABLE IT WAS
/// CLAIMED IN — the ordinary `inflight` table.
///
/// `begin_inline_flow_return_flight` inserts into `inflight`, so the
/// publish must retire from `inflight`. Retiring from the
/// `independent_inflight` table instead leaves the completed entry
/// resident forever. The damage is not a stale value (a joiner does fork
/// on a generation bump) — it is that the key can NEVER re-warm:
/// admission finds the resident entry and takes the joiner branch on
/// every later demand, so once the warm family is dropped (the global
/// `memo_budget` FIFO drain) the flight table becomes an ungated shadow
/// cache with no generation gate, no bounded retention, and no
/// reverse-index participation, holding the member's `Arc` payload and
/// full carrier forever.
///
/// `retained_claimed_flight_keys_for_tests` cannot see this: it filters
/// on `completed.is_none()`, and the leaked entry is COMPLETED. The
/// probe here deliberately ignores completion state.
#[test]
fn flow_return_drained_member_retires_from_the_table_it_claimed() {
    for (first, second) in [("scCleanA", "scCleanB"), ("scCleanB", "scCleanA")] {
        let host = make_scc_host();
        let (_, first_candidates, _) = scc_expect_value(&host, first);
        assert_eq!(first_candidates, 1, "{first} admits warm as machinery root");

        let resident = with_dispatch(&host, |dispatch| {
            dispatch.graph().resident_flight_keys_for_tests()
        });
        assert!(
            resident.is_empty(),
            "the closed component must leave NO resident flight entry \
             (demand order {first} → {second}), found {resident:?}"
        );

        // The re-warm direction: drop the member's warm family exactly as
        // the global `memo_budget` FIFO drain does, leaving the in-flight
        // table alone. A correctly retired flight lets the next demand
        // re-admit cold and warm again; a leaked one makes the key join a
        // corpse forever.
        with_dispatch(&host, |dispatch| {
            let key = SemanticQueryKey::FlowReturn(Box::new(scc_key(dispatch, second)));
            dispatch.graph().evict_family_for_tests(&key);
        });
        let (_, second_candidates, _) = scc_expect_value(&host, second);
        assert_eq!(
            second_candidates, 1,
            "{second} must RE-WARM after its family is dropped; a leaked flight \
             makes admission join the retained completed entry forever"
        );
    }
}

/// Every member of a resurrected flow SCC must publish a READABLE entry.
///
/// A member whose own evaluation failed `EmptyCycle` has no seed of its
/// own, so its evaluation records `MaterializedSet::empty()`. The
/// component discharge then RESURRECTS it to `Complete` from its hold
/// targets — but the resurrection copies only the outcome, so the
/// published entry keeps the empty materialised set. `cached_satisfies`
/// is an `.any(...)` over that set, so `∅` satisfies NOTHING: the
/// candidate occupies a slot, a reverse-index registration and a FIFO
/// budget admission while being permanently unreadable.
///
/// Candidate count alone does NOT discriminate — a zombie satisfies it.
/// The discriminating assertions are the recorded materialised set and
/// the actual warm read.
#[test]
fn flow_return_resurrected_member_publishes_a_readable_entry() {
    // The two-cycle: whichever member is demanded first, the other one
    // is the seedless `EmptyCycle` member the discharge resurrects.
    for (first, rest) in [
        ("scResA", ["scResB"].as_slice()),
        ("scResB", ["scResA"].as_slice()),
        // The three-cycle demanded at its only seeded member leaves BOTH
        // other members resurrected in the SAME drain.
        ("scRingX", ["scRingY", "scRingZ"].as_slice()),
    ] {
        let host = make_scc_host();
        let (first_result, _, _) = scc_expect_value(&host, first);
        assert_eq!(
            first_result.degradation(),
            None,
            "{first} closes its component cleanly"
        );

        for name in rest {
            let (materialized, warm) = with_dispatch(&host, |dispatch| {
                let key = scc_key(dispatch, name);
                let materialized = dispatch.graph().entry_satisfied_projection_for_tests(
                    &SemanticQueryKey::FlowReturn(Box::new(key.clone())),
                );
                let warm = dispatch.graph().get_flow_return_result(dispatch.ctx, &key);
                (materialized, warm)
            });
            let materialized =
                materialized.unwrap_or_else(|| panic!("{name} must publish a candidate"));
            assert!(
                !materialized.is_empty(),
                "{name} (resurrected in {first}'s drain) must record the point its \
                 published result actually serves — an empty set satisfies nothing"
            );
            assert!(
                warm.is_some(),
                "{name} (resurrected in {first}'s drain) must be WARM-READABLE; \
                 an entry that occupies a slot but can never be read is a zombie"
            );
        }
    }
}

/// An SCC's deferred members must publish ONLY onto a root candidate
/// that is still live, and they must publish ATOMICALLY.
///
/// The relation publisher takes the root's admission token and, under
/// the SAME `entries` lock it will publish with, refuses when the root
/// family no longer holds that exact candidate. Three of the four
/// member-publish sites had no such fence: a member drained onto a root
/// an invalidation had already swept would publish anyway and serve a
/// live warm read from a component whose root no longer exists.
///
/// The invalidation abort sweep cannot cover this — `affected_pairs`
/// comes from the reverse-index drain, and a deferred, never-published
/// member's flight holds no registration.
///
/// Atomicity is the second half: publishing each member under its own
/// `entries` hold lets an invalidation land BETWEEN members and leave a
/// partially-published component. The contract is that a superseded root
/// releases the ENTIRE component with ZERO member publication, so this
/// drives a THREE-member component and requires both members to refuse
/// together.
///
/// Mutation recipe: dropping the root-witness check from the batched
/// publish restores `publish -> true` with both members warm-readable
/// under a swept root.
#[test]
fn flow_scc_members_never_publish_onto_a_superseded_root() {
    for invalidate_root in [false, true] {
        let host = make_scc_host();
        // Warm the component so the root's published carrier exists and
        // both members carry real results.
        let (_, root_candidates, _) = scc_expect_value(&host, "scRingX");
        assert_eq!(root_candidates, 1, "the root must warm before the drain");

        with_dispatch(&host, |dispatch| {
            let graph = dispatch.graph();
            let root_key = scc_key(dispatch, "scRingX");
            let root_query = SemanticQueryKey::FlowReturn(Box::new(root_key.clone()));
            let carrier = graph
                .published_carrier_for_tests(&root_query)
                .expect("the root publishes a carrier");

            // Re-stage both members exactly as the SCC drain does: drop
            // the warm entry, then claim the ordinary family flight.
            let mut staged = Vec::new();
            for name in ["scRingY", "scRingZ"] {
                let key = scc_key(dispatch, name);
                let query = SemanticQueryKey::FlowReturn(Box::new(key.clone()));
                let result = graph
                    .get_flow_return_result(dispatch.ctx, &key)
                    .unwrap_or_else(|| panic!("{name} must be warm before re-staging"));
                let materialized = graph
                    .entry_satisfied_projection_for_tests(&query)
                    .unwrap_or_else(|| panic!("{name} must record its materialised point"));
                graph.evict_family_for_tests(&query);
                let flight = graph
                    .begin_inline_flow_return_flight(&key)
                    .unwrap_or_else(|| panic!("{name} must claim its family flight"));
                staged.push((name, key, query, result, materialized, flight));
            }

            // The invalidation lands with both flights already claimed —
            // the exact production ordering, and the one the abort sweep
            // cannot see.
            if invalidate_root {
                graph.invalidate_canonical(SCC_CANONICAL);
                assert_eq!(
                    graph.slot_candidate_count_for_tests(&root_query),
                    0,
                    "the root candidate must be swept before the drain resumes"
                );
            }

            let pending: Vec<_> = staged
                .iter()
                .cloned()
                .map(|(_, key, _, result, materialized, flight)| {
                    crate::semantic_query_memo::PendingFlowReturnMember {
                        key,
                        result,
                        materialized,
                        flight,
                    }
                })
                .collect();
            let published_any = graph.publish_scc_members_fenced(
                Some(dispatch.ctx),
                &crate::semantic_query_memo::SccRootWitness::flow_return(
                    root_key.clone(),
                    carrier.admission_seq,
                ),
                &carrier.read_set_signature,
                &carrier.self_root_canonicals,
                carrier.validated_at_generation,
                Vec::new(),
                pending,
                Vec::new(),
            );

            for (name, key, query, ..) in &staged {
                let candidates = graph.slot_candidate_count_for_tests(query);
                let warm = graph.get_flow_return_result(dispatch.ctx, key).is_some();
                if invalidate_root {
                    assert!(
                        !published_any,
                        "no member may publish onto a swept root ({name})"
                    );
                    assert_eq!(
                        candidates, 0,
                        "{name} must leave no candidate behind a swept root"
                    );
                    assert!(
                        !warm,
                        "{name} must not serve a warm read from a component whose root is gone"
                    );
                } else {
                    assert!(published_any, "the positive control must publish ({name})");
                    assert_eq!(candidates, 1, "{name} publishes onto the live root");
                    assert!(warm, "{name} serves its published result warm");
                }
            }

            let retained = graph.retained_claimed_flight_keys_for_tests();
            assert!(
                retained.is_empty(),
                "every member flight must be released either way: {retained:?}"
            );
            let resident = graph.resident_flight_keys_for_tests();
            assert!(
                resident.is_empty(),
                "every member flight must be retired either way: {resident:?}"
            );
        });
    }
}
