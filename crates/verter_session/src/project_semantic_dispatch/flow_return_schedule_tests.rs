//! The flow-return callee schedule: a finite call chain consumes connected
//! WORK — never native stack or connected-query depth per level — and
//! evaluates exactly as the recursive path does.
//!
//! Every expected answer is measured against the pinned oracle
//! (TypeScript 7.0.2, `tsc --declaration --emitDeclarationOnly --strict`)
//! and recorded on the test that asserts it.

use super::*;
use crate::project_semantic_dispatch::connected_demand::{
    MAX_CONNECTED_PROJECTION_WORK, MAX_CONNECTED_QUERY_DEPTH,
};
use crate::project_semantic_dispatch::flow_return::schedule::disable_flow_return_schedule_for_tests;
use crate::semantic_query::PartialReasonSet;
use verter_type_expr::ObjectMember;

const PATH: &str = "/ws/cov/schedule/chain.ts";

/// The chain the connected-query depth gate was stated on:
///
/// ```ts
/// function c0<T>(x: T) { return { v: x, tag: "c" as const }; }
/// function cN<T>(x: T) { return c(N-1)(x); }
/// export function witness(v: number | string) { return c(levels-1)(v); }
/// ```
fn witness_chain(levels: usize) -> String {
    let mut source = "function c0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function c{level}<T>(x: T) {{ return c{}(x); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return c{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A non-generic chain: `p0(x: number)` returns the tagged value and
/// `pN(x: number)` returns `p(N-1)(x)`.
fn plain_chain(levels: usize) -> String {
    let mut source =
        "function p0(x: number) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function p{level}(x: number) {{ return p{}(x); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number) {{ return p{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A chain of generic arrow functions bound to `const`s.
fn arrow_chain(levels: usize) -> String {
    let mut source = "const a0 = <T,>(x: T) => ({ v: x, tag: \"c\" as const });\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "const a{level} = <T,>(x: T) => a{}(x);\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return a{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A chain whose every level calls the next through a local arrow
/// function: `lN<T>(x: T) { const f = (y: T) => l(N-1)(y); return f(x); }`.
fn local_arrow_chain(levels: usize) -> String {
    let mut source = "function l0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function l{level}<T>(x: T) {{ const f = (y: T) => l{}(y); return f(x); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return l{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A chain generator: the source of a chain `levels` calls long.
type ChainSource = fn(usize) -> String;

/// Evaluate `witness` in a fresh host holding `source` alone, under the
/// given connected-demand caps: its outcome and the connected work it
/// charged.
fn witness_under_caps(source: &str, work: usize, depth: u16) -> (Outcome, usize) {
    let host = host_with(&[(PATH, source)]);
    with_dispatch(&host, |dispatch| {
        dispatch.set_connected_limits_for_tests(work, depth);
        let key = key_of(dispatch, PATH, "witness");
        let outcome = eval_key_on(&host, dispatch, key);
        (outcome, dispatch.connected_demand.work_used_for_tests())
    })
}

fn witness_outcome(source: &str) -> Outcome {
    witness_under_caps(
        source,
        MAX_CONNECTED_PROJECTION_WORK,
        MAX_CONNECTED_QUERY_DEPTH,
    )
    .0
}

/// The smallest connected-query depth cap under which `source`'s witness
/// answers a value.
fn depth_needed(source: &str) -> u16 {
    (1..=MAX_CONNECTED_QUERY_DEPTH)
        .find(|&depth| {
            matches!(
                witness_under_caps(source, MAX_CONNECTED_PROJECTION_WORK, depth).0,
                Outcome::Value { .. }
            )
        })
        .expect("the witness answers under the production depth cap")
}

/// Run `work` on a thread with a `stack_bytes` native stack.
fn on_stack<R: Send + 'static>(stack_bytes: usize, work: impl FnOnce() -> R + Send + 'static) -> R {
    let worker = std::thread::Builder::new()
        .stack_size(stack_bytes)
        .spawn(work)
        .expect("spawn the chain worker");
    match worker.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// A clean, warm-admitted `{ v: <arms>; tag: "c" }`.
#[track_caller]
fn assert_tagged_value(outcome: &Outcome, arms: &[TypeExpr]) {
    let Outcome::Value {
        ty: TypeExpr::Object(object),
        degradation: None,
        candidates: 1,
    } = outcome
    else {
        panic!("expected a clean, admitted object: {outcome:?}");
    };
    let property = |name: &str| {
        object.properties.iter().find_map(|member| match member {
            ObjectMember::Property(property) if property.key == name.into() => Some(&property.ty),
            _ => None,
        })
    };
    assert_eq!(
        property("tag"),
        Some(&TypeExpr::Literal(LiteralValue::String("c".to_string()))),
        "{outcome:?}"
    );
    match property("v") {
        Some(TypeExpr::Union(members)) => {
            assert!(
                members.len() == arms.len() && arms.iter().all(|arm| members.contains(arm)),
                "{outcome:?}"
            );
        }
        Some(single) => assert_eq!(std::slice::from_ref(single), arms, "{outcome:?}"),
        None => panic!("no `v` member: {outcome:?}"),
    }
    assert_eq!(object.properties.len(), 2, "{outcome:?}");
}

/// A 128-level generic call chain answers exactly like the short chain,
/// under the same connected-query depth cap the short chain needs.
///
/// Oracle (the pinned TypeScript 7.0.2, `--strict`): `witness` is
/// `{ v: string | number; tag: "c"; }` for the 128-level chain and for the
/// three-level one alike.
///
/// Each level's body demands its callee's return where the call sits —
/// for a generic call beneath the `TypeOf` / `LowerLocator` queries that
/// lower `typeof callee` — so the recursive path nested two connected
/// queries per level and refused a chain past eleven levels. The schedule
/// evaluates the callees bottom-up first: the depth the witness needs no
/// longer depends on the chain's length, and it answers well below the
/// production cap.
#[test]
fn a_128_level_generic_chain_needs_no_more_query_depth_than_a_short_one() {
    on_stack(8 << 20, || {
        let short = witness_chain(3);
        let long = witness_chain(128);
        let short_outcome = witness_outcome(&short);
        assert_tagged_value(&short_outcome, &[number(), string()]);
        let depth = depth_needed(&short);
        assert!(
            depth < MAX_CONNECTED_QUERY_DEPTH / 2,
            "the short chain needs a query depth of {depth}"
        );
        assert_eq!(
            witness_under_caps(&long, MAX_CONNECTED_PROJECTION_WORK, depth).0,
            short_outcome,
            "the 128-level chain answers like the short one under the short one's \
             depth cap ({depth})"
        );
        assert_eq!(depth_needed(&long), depth);
    });
}

/// The 128-level chain runs on a native stack no deeper than the short
/// chain needs, so no native stack is consumed per level.
///
/// Measured on the unoptimized test profile: the three-level and the
/// 128-level chains each need about 400 KiB; without the schedule the
/// three-level chain needs about 850 KiB and every further level about
/// 250 KiB more (eleven levels: 2.7 MiB), which the 512 KiB here cannot
/// hold.
///
/// Oracle: as for
/// [`a_128_level_generic_chain_needs_no_more_query_depth_than_a_short_one`].
#[test]
fn a_128_level_generic_chain_runs_on_the_short_chains_native_stack() {
    let (long, short) = on_stack(512 << 10, || {
        (
            witness_outcome(&witness_chain(128)),
            witness_outcome(&witness_chain(3)),
        )
    });
    assert_tagged_value(&short, &[number(), string()]);
    assert_eq!(long, short);
}

/// With a connected-work budget the 128-level chain cannot meet, the
/// witness ends through the ordinary typed budget incompleteness — the
/// work rail, partial and non-admitted — never through the query-depth
/// rail and never by exhausting the stack.
#[test]
fn a_deep_chain_over_a_reduced_budget_ends_on_the_work_rail() {
    let (read, used, candidates) = on_stack(8 << 20, || {
        let host = host_with(&[(PATH, witness_chain(128).as_str())]);
        with_dispatch(&host, |dispatch| {
            dispatch.set_connected_limits_for_tests(1_000, MAX_CONNECTED_QUERY_DEPTH);
            let key = key_of(dispatch, PATH, "witness");
            let read = dispatch
                .execute_via_cold_build_helper(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
            let candidates = dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
            (
                (
                    read.result_is_partial,
                    read.cache_suppress,
                    read.partial_reasons,
                ),
                dispatch.connected_demand.work_used_for_tests(),
                candidates,
            )
        })
    });
    let (partial, suppressed, reasons) = read;
    assert!(
        partial && suppressed,
        "a budget trip is partial and never admitted"
    );
    assert!(
        reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "the chain ends on the work rail: {reasons:?}"
    );
    assert!(
        !reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT),
        "the chain never reaches the depth rail: {reasons:?}"
    );
    assert_eq!(used, 1_000, "the whole budget was spent on the chain");
    assert_eq!(candidates, 0, "nothing was admitted warm");
}

/// Chains through other call shapes run under the short chain's depth cap
/// and on a fixed small native stack too.
///
/// Measured on the unoptimized test profile, the three-level and the
/// 128-level chains need the same stack: about 210 KiB for the non-generic
/// chain, 400 KiB for the `const` arrow chain and 525 KiB for the local
/// arrow chain, so all three run on 1 MiB, where a recursive 128-level
/// chain of any of them would need tens of MiB.
///
/// Oracle: `witness` is `{ v: number; tag: "c"; }` for the non-generic
/// chain, and `{ v: string | number; tag: "c"; }` for the chain of generic
/// `const` arrow functions and for the chain through local arrow
/// functions.
#[test]
fn chains_through_other_call_shapes_consume_no_depth_per_level() {
    let shapes: [(&str, ChainSource, Vec<TypeExpr>); 3] = [
        ("non-generic", plain_chain, vec![number()]),
        ("const arrow", arrow_chain, vec![number(), string()]),
        ("local arrow", local_arrow_chain, vec![number(), string()]),
    ];
    for (shape, chain, arms) in shapes {
        let (long, short, depth) = on_stack(8 << 20, move || {
            let short_source = chain(3);
            let depth = depth_needed(&short_source);
            (
                witness_under_caps(&chain(128), MAX_CONNECTED_PROJECTION_WORK, depth).0,
                witness_outcome(&short_source),
                depth,
            )
        });
        assert_tagged_value(&short, &arms);
        assert_eq!(
            long, short,
            "the 128-level {shape} chain answers like the short one under its depth cap \
             ({depth})"
        );
        let on_small_stack = on_stack(1 << 20, move || witness_outcome(&chain(128)));
        assert_eq!(
            on_small_stack, short,
            "the 128-level {shape} chain runs on a 1 MiB stack"
        );
    }
}

/// Every recursive component evaluates exactly as the recursive path
/// evaluates it — the same answer from the same connected work — because
/// the schedule never evaluates a member of a cycle out of order: a cycle
/// through a frame in flight is left to that frame's body, and a cycle
/// among the scheduled callees is evaluated from its first-discovered
/// member, where the re-entry intercept holds each back-edge.
///
/// Oracle: `selfRec` is `string` (tsc excludes the circular reference of
/// a directly self-recursive return); the mutually recursive `ping` /
/// `pong` pairs are `TS7023` (implicit `any`), so their answers are the
/// recursive path's fixed point, which the schedule must reproduce.
#[test]
fn recursive_components_evaluate_exactly_as_the_recursive_path() {
    const SOURCE: &str = "\
function t0(n: number) { return { depth: n }; }\n\
function t1(n: number) { return t0(n); }\n\
function u0(n: number) { return { depth: n }; }\n\
function u1(n: number) { return u0(n); }\n\
function selfRec(n: number) { if (n <= 0) return \"done\"; return selfRec(n - 1); }\n\
function s1(n: number) { return selfRec(n); }\n\
export function wSelf(n: number) { return s1(n); }\n\
function ping(n: number) { if (n <= 0) return t1(n); return pong(n - 1); }\n\
function pong(n: number) { if (n <= 1) return u1(n); return ping(n - 1); }\n\
export function wCycle(n: number) { return ping(n); }\n\
function tick(n: number) { if (n <= 0) return t1(n); return tock(n - 1); }\n\
function tock(n: number) { if (n <= 1) return u1(n); return tick(n - 1); }\n\
export function tickRoot(n: number) { return tick(n); }\n";
    let evaluate = |name: &'static str, scheduled: bool| {
        on_stack(8 << 20, move || {
            let _recursive = (!scheduled).then(disable_flow_return_schedule_for_tests);
            let host = host_with(&[(PATH, SOURCE)]);
            with_dispatch(&host, |dispatch| {
                let key = key_of(dispatch, PATH, name);
                let outcome = eval_key_on(&host, dispatch, key);
                (outcome, dispatch.connected_demand.work_used_for_tests())
            })
        })
    };
    // `tick` is demanded at the member itself: its cycle runs through the
    // frame in flight.
    for name in ["wSelf", "wCycle", "tick", "tickRoot"] {
        let recursive = evaluate(name, false);
        let scheduled = evaluate(name, true);
        assert_eq!(
            scheduled, recursive,
            "`{name}` must evaluate exactly as the recursive path does"
        );
    }
    assert_eq!(
        evaluate("wSelf", true).0,
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}
