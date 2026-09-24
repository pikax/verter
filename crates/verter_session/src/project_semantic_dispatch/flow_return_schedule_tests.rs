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

/// A chain across `levels` modules: module `k` imports `c(k-1)` from
/// module `k-1` and exports `c(k)`, calling it; the last module exports the
/// witness. `generic` gives every level a `<T>` clause.
fn module_chain(levels: usize, generic: bool) -> Vec<(String, String)> {
    (0..levels)
        .map(|level| {
            let mut source = String::new();
            let (clause, param) = if generic { ("<T>", "T") } else { ("", "number") };
            if level == 0 {
                source.push_str(&format!(
                    "export function c0{clause}(x: {param}) {{ return {{ v: x, tag: \"c\" as const }}; }}\n"
                ));
            } else {
                let previous = level - 1;
                source.push_str(&format!(
                    "import {{ c{previous} }} from \"./m{previous}\";\n\
                     export function c{level}{clause}(x: {param}) {{ return c{previous}(x); }}\n"
                ));
            }
            if level == levels - 1 {
                let witness = if generic { "number | string" } else { "number" };
                source.push_str(&format!(
                    "export function witness(v: {witness}) {{ return c{level}(v); }}\n"
                ));
            }
            (format!("/ws/cov/schedule/m{level}.ts"), source)
        })
        .collect()
}

/// The witness of a module chain, under the given query-depth cap.
fn module_witness_under_depth(files: &[(String, String)], depth: u16) -> Outcome {
    let sources: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let host = host_with(&sources);
    let entry = files.last().expect("a chain has a last module").0.as_str();
    with_dispatch(&host, |dispatch| {
        dispatch.set_connected_limits_for_tests(MAX_CONNECTED_PROJECTION_WORK, depth);
        let key = key_of(dispatch, entry, "witness");
        eval_key_on(&host, dispatch, key)
    })
}

/// A chain across modules — every level a callee imported from the module
/// before — needs no more query depth, and no more native stack, than a
/// short one: the schedule resolves an imported callee through the
/// module's imports exactly as the call rail's `typeof callee` lowering
/// does, and evaluates it bottom-up.
///
/// Oracle: `witness` is `{ v: string | number; tag: "c"; }` for the
/// generic chain (measured across 31 modules) and `{ v: number; tag: "c"; }`
/// for the non-generic one (measured across 32).
///
/// Without the schedule, each level nested the imported callee's
/// evaluation inside its `typeof` lowering, two connected queries deep, and
/// the depth guard refused a chain past eleven modules.
#[test]
fn a_chain_across_modules_needs_no_more_query_depth_than_a_short_one() {
    for (generic, arms) in [(true, vec![number(), string()]), (false, vec![number()])] {
        let (long, short, depth) = on_stack(8 << 20, move || {
            let short = module_chain(4, generic);
            let depth = (1..=MAX_CONNECTED_QUERY_DEPTH)
                .find(|&depth| {
                    matches!(
                        module_witness_under_depth(&short, depth),
                        Outcome::Value { .. }
                    )
                })
                .expect("the short chain answers under the production depth cap");
            (
                module_witness_under_depth(&module_chain(32, generic), depth),
                module_witness_under_depth(&short, MAX_CONNECTED_QUERY_DEPTH),
                depth,
            )
        });
        assert_tagged_value(&short, &arms);
        assert!(
            depth < MAX_CONNECTED_QUERY_DEPTH / 4,
            "the short chain needs a query depth of {depth}"
        );
        assert_eq!(
            long, short,
            "the 32-module chain answers like the 4-module one under its depth cap ({depth})"
        );
        let on_small_stack = on_stack(512 << 10, move || {
            module_witness_under_depth(&module_chain(32, generic), MAX_CONNECTED_QUERY_DEPTH)
        });
        assert_eq!(
            on_small_stack, short,
            "the 32-module chain runs on the short chain's stack"
        );
    }
}

/// A new instantiation of a chain whose uninstantiated answers are already
/// warm — a second call site after the first request warmed the chain —
/// runs on the short chain's stack too.
///
/// The uninstantiated frames never evaluate on the second request, so no
/// record of what their call resolution demanded exists there: the
/// schedule reads the instantiation each level demands from the two
/// signatures the call executor reads, because every level FORWARDS its
/// own binder. Without that, each level's instantiation nested one native
/// evaluation per level with no query boundary between them.
///
/// Oracle (the pinned TypeScript 7.0.2, `--strict`, over the same 128-level
/// chain): `second(v: boolean)` is `{ v: boolean; tag: "c"; }`.
#[test]
fn a_new_instantiation_of_a_warm_chain_runs_on_the_short_chains_native_stack() {
    let mut source = witness_chain(128);
    source.push_str("export function second(v: boolean) { return c127(v); }\n");
    let host = host_with(&[(PATH, source.as_str())]);
    let warm = {
        let host = Arc::clone(&host);
        on_stack(8 << 20, move || {
            with_dispatch(&host, |dispatch| {
                eval_key_on(&host, dispatch, key_of(dispatch, PATH, "witness"))
            })
        })
    };
    assert_tagged_value(&warm, &[number(), string()]);
    let second = on_stack(512 << 10, move || {
        with_dispatch(&host, |dispatch| {
            eval_key_on(&host, dispatch, key_of(dispatch, PATH, "second"))
        })
    });
    assert_tagged_value(&second, &[TypeExpr::Primitive(PrimitiveName::Boolean)]);
}

/// The second request over a warm chain of `levels` local-arrow levels —
/// the first request evaluates `witness`, the second `second(v: boolean)`,
/// with the schedule on or off for the second request: the first request's
/// answer, the second's value (projected only when it is one), whether it
/// ended partial and suppressed, its partial rails, how many warm
/// candidates it admitted, and the connected work it charged.
struct WarmSecond {
    warm: Outcome,
    value: Option<Outcome>,
    partial: bool,
    reasons: PartialReasonSet,
    candidates: usize,
    work: usize,
}

fn warm_local_chain_second(levels: usize, scheduled: bool) -> WarmSecond {
    let mut source = local_arrow_chain(levels);
    source.push_str(&format!(
        "export function second(v: boolean) {{ return l{}(v); }}\n",
        levels - 1
    ));
    let host = host_with(&[(PATH, source.as_str())]);
    let warm = with_dispatch(&host, |dispatch| {
        eval_key_on(&host, dispatch, key_of(dispatch, PATH, "witness"))
    });
    let _recursive = (!scheduled).then(disable_flow_return_schedule_for_tests);
    with_dispatch(&host, |dispatch| {
        let key = key_of(dispatch, PATH, "second");
        let read = dispatch
            .execute_via_cold_build_helper(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
        let work = dispatch.connected_demand.work_used_for_tests();
        let candidates = dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key.clone())));
        let value =
            matches!(read.value, QueryResult::Value(_)).then(|| eval_key_on(&host, dispatch, key));
        WarmSecond {
            warm,
            value,
            partial: read.result_is_partial && read.cache_suppress,
            reasons: read.partial_reasons,
            candidates,
            work,
        }
    })
}

/// Native recursion the schedule does not predict ends in the typed depth
/// refusal, never in a stack overflow: an inline flow evaluation that would
/// nest past the connected demand's depth bound is refused with
/// `CONNECTED_QUERY_DEPTH_LIMIT`, partial and never admitted.
///
/// The witness is a new instantiation of a warm chain whose every level
/// calls the next through a local arrow function, evaluated with the
/// schedule off, so each level nests one evaluation natively with no query
/// boundary between them. Below the bound (16 levels) it answers; past it
/// (256 levels, which without the bound overflows the 8 MiB production
/// worker stack in an unoptimized build) it is refused, typed.
///
/// Oracle (the pinned TypeScript 7.0.2, `--strict`): `second(v: boolean)`
/// is `{ v: boolean; tag: "c"; }` at both lengths (measured at 64), so the
/// refusal is a typed gap, not an answer.
#[test]
fn an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal() {
    let short = on_stack(8 << 20, || warm_local_chain_second(16, false));
    assert_tagged_value(&short.warm, &[number(), string()]);
    assert!(
        short.value.is_some() && !short.partial && short.candidates == 1,
        "a 16-level chain stays within the bound and answers: {:?}",
        short.reasons
    );
    let WarmSecond {
        warm,
        value,
        partial,
        reasons,
        candidates,
        ..
    } = on_stack(8 << 20, || warm_local_chain_second(256, false));
    let answered = value.is_some();
    assert_tagged_value(&warm, &[number(), string()]);
    assert!(
        !answered && partial && candidates == 0,
        "a 256-level unpredicted chain ends typed, partial and never admitted \
         (answered: {answered}, partial: {partial}, candidates: {candidates})"
    );
    assert!(
        reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT),
        "the refusal is the depth rail's: {reasons:?}"
    );
}

/// A chain whose every level wraps its callee's call in a generic `id`:
/// `cN<T>(x: T) { return id(c(N-1)(x)); }` — each level's callee is called
/// inside another call's argument.
fn nested_argument_chain(levels: usize) -> String {
    let mut source = "function id<T>(x: T) { return x; }\n\
                      function c0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n"
        .to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function c{level}<T>(x: T) {{ return id(c{}(x)); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return c{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A chain whose every level reads its callee's return through a TYPE
/// position: `tN(x: number) { let r!: ReturnType<typeof t(N-1)>; return r; }`.
fn type_position_chain(levels: usize) -> String {
    let mut source =
        "function t0(x: number) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function t{level}(x: number) {{ let r!: ReturnType<typeof t{}>; return r; }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number) {{ return t{}(v); }}\n",
        levels - 1
    ));
    source
}

/// One cold read of a function: whether its answer, reduced to the
/// altitude the checker prints at, is the checker's print; whether it ended
/// partial; its partial rails; the warm candidates it admitted; and the
/// connected work the evaluation charged before any reduction.
struct ColdRead {
    printed: Result<(), String>,
    partial: bool,
    reasons: PartialReasonSet,
    candidates: usize,
    work: usize,
}

/// Read `name` cold and compare its answer with the checker's print
/// `expected` through the shared checker-syntax projection.
fn cold_read(
    dispatch: &ProjectSemanticDispatch<'_>,
    path: &str,
    name: &str,
    expected: &str,
) -> ColdRead {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    let key = key_of(dispatch, path, name);
    let read =
        dispatch.execute_via_cold_build_helper(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
    let work = dispatch.connected_demand.work_used_for_tests();
    let candidates = dispatch
        .graph()
        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
    let printed = match &read.value {
        QueryResult::Value(SemanticQueryValue::FlowReturn(result))
            if result.degradation().is_none() =>
        {
            let reduced = dispatch
                .normalize_node_keeping_declaration_refs_for_tests(
                    result.return_type(),
                    crate::semantic_query::ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                )
                .into_complete_node();
            let parsed = checker_syntax::parse(expected)
                .unwrap_or_else(|error| panic!("`{expected}` must parse: {error}"));
            match reduced {
                Some(node) if checker_syntax::matches_node(dispatch, node, &parsed, 0) => Ok(()),
                Some(node) => Err(format!(
                    "`{name}`: expected `{expected}`, measured `{}`",
                    render_node(dispatch, node, 0)
                )),
                None => Err(format!("`{name}`: the reduction did not complete")),
            }
        }
        other => Err(format!("`{name}` answered no clean value: {other:?}")),
    };
    ColdRead {
        printed,
        partial: read.result_is_partial,
        reasons: read.partial_reasons,
        candidates,
        work,
    }
}

/// `name` in a fresh host holding `source` alone, read cold.
fn cold_read_of(source: &str, name: &str, expected: &str) -> ColdRead {
    let host = host_with(&[(PATH, source)]);
    with_dispatch(&host, |dispatch| cold_read(dispatch, PATH, name, expected))
}

/// A clean, warm-admitted answer equal to the checker's print.
#[track_caller]
fn assert_answers_as_the_checker(read: &ColdRead) {
    if let Err(message) = &read.printed {
        panic!("{message} (rails: {:?})", read.reasons);
    }
    assert!(
        !read.partial && read.candidates == 1,
        "a clean, admitted answer (partial: {}, candidates: {}, rails: {:?})",
        read.partial,
        read.candidates,
        read.reasons
    );
}

/// The connected work one more level adds to a chain at each of `levels`.
fn work_per_level(chain: ChainSource, expected: &str, levels: &[usize]) -> Vec<usize> {
    levels
        .iter()
        .map(|&levels| {
            let shorter = cold_read_of(&chain(levels), "witness", expected);
            let longer = cold_read_of(&chain(levels + 1), "witness", expected);
            assert_answers_as_the_checker(&shorter);
            assert_answers_as_the_checker(&longer);
            longer.work - shorter.work
        })
        .collect()
}

/// The smallest connected-query depth cap under which a chain's witness
/// answers the checker's print.
fn depth_answering(source: &str, expected: &str) -> u16 {
    (1..=MAX_CONNECTED_QUERY_DEPTH)
        .find(|&depth| {
            let host = host_with(&[(PATH, source)]);
            with_dispatch(&host, |dispatch| {
                dispatch.set_connected_limits_for_tests(MAX_CONNECTED_PROJECTION_WORK, depth);
                cold_read(dispatch, PATH, "witness", expected)
                    .printed
                    .is_ok()
            })
        })
        .expect("the witness answers under the production depth cap")
}

/// `witness` over `source`, read cold under the connected-query depth cap
/// `depth`.
fn witness_under_depth(source: &str, expected: &str, depth: u16) -> ColdRead {
    let host = host_with(&[(PATH, source)]);
    with_dispatch(&host, |dispatch| {
        dispatch.set_connected_limits_for_tests(MAX_CONNECTED_PROJECTION_WORK, depth);
        cold_read(dispatch, PATH, "witness", expected)
    })
}

/// A call nested inside another call's argument is evaluated in the frame
/// it is written in: its callee, its own arguments and the bindings they
/// read are the frame's, and its value takes the frame's call carrier.
///
/// Oracle (the pinned TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`, identical under all four `strictNullChecks` x
/// `noImplicitAny` settings):
///
/// | function | declared return |
/// |---|---|
/// | `witness` over the four-level `id(c(N-1)(x))` chain | `{ v: string \| number; tag: "c"; }` |
/// | `nested(v: number \| string)` = `id(id(id(c0(v))))` | `{ v: string \| number; tag: "c"; }` |
/// | `w1()` = `id(g())`, `g()` returning `"a"` | `string` |
/// | `w2()` = `id(gc())`, `gc()` returning `"a" as const` | `"a"` |
/// | `w3(n: number)` = `id(h(n))`, `h` returning `"a"` or `"b"` | `"a" \| "b"` |
/// | `w4()` = `const r = id(g()); return r;` | `string` |
/// | `w5(n: number)` = `pair(num(n), id(n))` | `{ a: number; b: number; }` |
/// | `w6(s: string)` = `id(id(id(s)))` | `string` |
/// | `w7(n?: number)` = `id(num(n ?? 1))` | `number` |
///
/// Evaluated in the file's owner scope instead, the frame's `x`, `v`, `n`
/// and `s` are unbound, so `v` answered a semantic miss.
#[test]
fn a_call_in_another_calls_argument_evaluates_in_its_own_frame() {
    assert_answers_as_the_checker(&cold_read_of(
        &nested_argument_chain(4),
        "witness",
        "{ v: string | number; tag: \"c\"; }",
    ));
    const SOURCE: &str = "\
function id<T>(x: T) { return x; }\n\
function pair<A, B>(a: A, b: B) { return { a, b }; }\n\
function c0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n\
function g() { return \"a\"; }\n\
function gc() { return \"a\" as const; }\n\
function h(n: number) { return n > 0 ? \"a\" : \"b\"; }\n\
function num(n: number) { return n; }\n\
export function nested(v: number | string) { return id(id(id(c0(v)))); }\n\
export function w1() { return id(g()); }\n\
export function w2() { return id(gc()); }\n\
export function w3(n: number) { return id(h(n)); }\n\
export function w4() { const r = id(g()); return r; }\n\
export function w5(n: number) { return pair(num(n), id(n)); }\n\
export function w6(s: string) { return id(id(id(s))); }\n\
export function w7(n?: number) { return id(num(n ?? 1)); }\n";
    for (name, expected) in [
        ("nested", "{ v: string | number; tag: \"c\"; }"),
        ("w1", "string"),
        ("w2", "\"a\""),
        ("w3", "\"a\" | \"b\""),
        ("w4", "string"),
        ("w5", "{ a: number; b: number; }"),
        ("w6", "string"),
        ("w7", "number"),
    ] {
        assert_answers_as_the_checker(&cold_read_of(SOURCE, name, expected));
    }
}

/// A 200-level chain whose every edge is a call inside another call's
/// argument answers on the default test stack, under the connected-query
/// depth cap a three-level chain needs: the schedule evaluates each
/// argument's callee bottom-up, exactly as it does a direct call's.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings): `witness` is `{ v: string | number; tag:
/// "c"; }` over the 200-level chain.
///
/// Left to the recursive path, each level nests two connected queries and
/// the depth guard refuses the chain from twelve levels.
#[test]
fn a_200_level_nested_argument_chain_answers_on_the_default_stack() {
    let expected = "{ v: string | number; tag: \"c\"; }";
    let depth = depth_answering(&nested_argument_chain(3), expected);
    assert_answers_as_the_checker(&witness_under_depth(
        &nested_argument_chain(200),
        expected,
        depth,
    ));
}

/// The nested-argument chain's connected work is linear in its length: one
/// more level costs the same at 16, 64 and 200 levels.
///
/// Oracle: as for
/// [`a_200_level_nested_argument_chain_answers_on_the_default_stack`], at
/// every length.
#[test]
fn a_nested_argument_chain_costs_the_same_work_per_level() {
    let per_level = work_per_level(
        nested_argument_chain,
        "{ v: string | number; tag: \"c\"; }",
        &[16, 64, 200],
    );
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}

/// A 200-level chain whose every edge reads the callee's return through a
/// TYPE position (`let r!: ReturnType<typeof t(N-1)>`) answers on the
/// default test stack, under the connected-query depth cap a three-level
/// chain needs: the schedule evaluates the function a type position's
/// `typeof` names bottom-up, as it does a callee.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings): `witness` is `{ v: number; tag: "c"; }` over
/// the 200-level chain.
///
/// Left to type lowering, each level nests two connected queries and the
/// depth guard refuses the chain from thirteen levels.
#[test]
fn a_200_level_type_position_chain_answers_on_the_default_stack() {
    let expected = "{ v: number; tag: \"c\"; }";
    let depth = depth_answering(&type_position_chain(3), expected);
    assert_answers_as_the_checker(&witness_under_depth(
        &type_position_chain(200),
        expected,
        depth,
    ));
}

/// The queries a cold read of `witness` over `source` dispatches, by kind.
fn dispatched_queries(source: &str) -> std::collections::BTreeMap<&'static str, usize> {
    use crate::project_semantic_dispatch::raise::{enable_dispatch_trace_for_test, DISPATCH_TRACE};
    let host = host_with(&[(PATH, source)]);
    with_dispatch(&host, |dispatch| {
        let _trace = enable_dispatch_trace_for_test();
        let read = cold_read(dispatch, PATH, "witness", "{ v: number; tag: \"c\"; }");
        assert_answers_as_the_checker(&read);
        DISPATCH_TRACE.with(|trace| {
            let mut counts = std::collections::BTreeMap::new();
            for kind in trace.borrow().iter() {
                *counts.entry(*kind).or_insert(0) += 1;
            }
            counts
        })
    })
}

/// The type-position chain lowers each level's `typeof` once: one more
/// level adds the same queries — one `TypeOf` and the `LowerLocator` it
/// lowers the function through — at 16, 64 and 200 levels, and the
/// schedule evaluates each level's return once, before the level that
/// reads it.
///
/// The chain's connected WORK is not linear: each level's `LowerLocator`
/// projects the function type `typeof` names, and that type's return is
/// the level below's `ReturnType<…>` carrier, whose argument is that
/// level's function type in turn. The view projection walks the whole
/// nested carrier chain beneath it — a walk that grows by one level per
/// level (about 1.5·N² units at N levels: 60,498 at 200, and the work rail
/// refuses the chain, typed, from 418 levels). The checker resolves a
/// `ReturnType` of a closed function type eagerly, so its chain stays
/// flat; this is the published-carrier representation's cost, not the
/// schedule's.
///
/// Oracle: as for
/// [`a_200_level_type_position_chain_answers_on_the_default_stack`], at
/// every length.
#[test]
fn a_type_position_chain_dispatches_the_same_queries_per_level() {
    let per_level: Vec<std::collections::BTreeMap<&'static str, usize>> = [16, 64, 200]
        .into_iter()
        .map(|levels| {
            let shorter = dispatched_queries(&type_position_chain(levels));
            let longer = dispatched_queries(&type_position_chain(levels + 1));
            longer
                .iter()
                .map(|(kind, count)| (*kind, count - shorter.get(kind).copied().unwrap_or(0)))
                .collect()
        })
        .collect();
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "queries per level: {per_level:?}"
    );
}

/// A new instantiation of a warm 200-level chain whose every level calls
/// the next through a local arrow function answers on the default test
/// stack: the arrow's parameter is annotated with the level's own binder,
/// so the schedule reads the instantiation each level demands from the
/// signatures, as it does for a direct call that forwards its binder.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings, over the same 200-level chain): `witness` is
/// `{ v: string | number; tag: "c"; }` and `second(v: boolean)` is `{ v:
/// boolean; tag: "c"; }`.
///
/// Left to the recursive path, each level nests one evaluation natively
/// and the nesting bound refuses the chain from 24 levels.
#[test]
fn a_new_instantiation_of_a_warm_200_level_local_arrow_chain_answers_on_the_default_stack() {
    let second = warm_local_chain_second(200, true);
    assert_tagged_value(&second.warm, &[number(), string()]);
    let Some(value) = &second.value else {
        panic!(
            "the second instantiation answers no value (rails: {:?})",
            second.reasons
        );
    };
    assert_tagged_value(value, &[TypeExpr::Primitive(PrimitiveName::Boolean)]);
    assert!(
        !second.partial && second.candidates == 1,
        "a clean, admitted answer (rails: {:?})",
        second.reasons
    );
}

/// A new instantiation of a warm local-arrow chain costs connected work
/// linear in the chain's length: one more level costs the same at 16, 64
/// and 200 levels.
///
/// Oracle: as for
/// [`a_new_instantiation_of_a_warm_200_level_local_arrow_chain_answers_on_the_default_stack`],
/// at every length.
#[test]
fn a_new_instantiation_of_a_warm_local_arrow_chain_costs_the_same_work_per_level() {
    let work = |levels: usize| {
        let second = warm_local_chain_second(levels, true);
        let Some(value) = &second.value else {
            panic!(
                "the {levels}-level instantiation answers no value (rails: {:?})",
                second.reasons
            );
        };
        assert_tagged_value(value, &[TypeExpr::Primitive(PrimitiveName::Boolean)]);
        second.work
    };
    let per_level: Vec<usize> = [16, 64, 200]
        .into_iter()
        .map(|levels| work(levels + 1) - work(levels))
        .collect();
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}
