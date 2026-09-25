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

/// The witness of a generic module chain `levels` modules long, in a fresh
/// host under the production caps: its outcome and the connected work its
/// demand charged.
fn module_chain_work(levels: usize) -> (Outcome, usize) {
    let files = module_chain(levels, true);
    let sources: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let host = host_with(&sources);
    let entry = files.last().expect("a chain has a last module").0.as_str();
    with_dispatch(&host, |dispatch| {
        let key = key_of(dispatch, entry, "witness");
        let outcome = eval_key_on(&host, dispatch, key);
        (outcome, dispatch.connected_demand.work_used_for_tests())
    })
}

/// Every module added to a generic chain costs the same connected work.
///
/// Each module's `c(k)` instantiates its callee over its OWN binder, a
/// type parameter of another file, so no two levels share an
/// instantiation. The instantiated return is read off the callee's
/// uninstantiated return under the instantiation, as the checker
/// instantiates a signature's return type, so each function's body is
/// evaluated once, generically, whatever instantiates it. Re-evaluating
/// the body under each instantiation instead would re-instantiate the whole
/// chain beneath every level, and the work would grow with the square of
/// the chain (measured with the read disabled: 25 / 50 / 100 / 200 modules cost
/// 4322 / 16772 / 66047 / 262097 units, against 397 / 797 / 1597 / 3197).
///
/// Oracle: as for [`a_generic_chain_across_201_modules_answers_without_a_refusal`].
#[test]
fn a_generic_chain_across_modules_costs_the_same_work_per_module() {
    on_stack(8 << 20, || {
        let (short, _) = module_chain_work(4);
        assert_tagged_value(&short, &[number(), string()]);
        let (nine, work_nine) = module_chain_work(9);
        let (ten, work_ten) = module_chain_work(10);
        let (eleven, work_eleven) = module_chain_work(11);
        for (levels, outcome) in [(9, nine), (10, ten), (11, eleven)] {
            assert_eq!(
                outcome, short,
                "a {levels}-module chain answers exactly like a four-module one"
            );
        }
        assert_eq!(
            work_eleven - work_ten,
            work_ten - work_nine,
            "every added module must cost the same connected work \
             ({work_nine} / {work_ten} / {work_eleven} at 9 / 10 / 11 modules)"
        );
        let per_module = work_eleven - work_ten;
        let (_, work_32) = module_chain_work(32);
        let (_, work_64) = module_chain_work(64);
        let (_, work_128) = module_chain_work(128);
        assert_eq!(
            (work_64 - work_32, work_128 - work_64),
            (32 * per_module, 64 * per_module),
            "every added module must cost the same connected work \
             ({work_32} / {work_64} / {work_128} at 32 / 64 / 128 modules)"
        );
    });
}

/// A generic chain across 201 modules answers the checker's value, clean
/// and admitted warm, under the production work budget.
///
/// Oracle (the pinned TypeScript 7.0.2, measured over the same 201
/// modules; `noImplicitAny` does not apply, every parameter is annotated):
///
/// | `strictNullChecks` | `noImplicitAny` | `witness` |
/// |---|---|---|
/// | on | on / off | `{ v: string \| number; tag: "c"; }` |
/// | off | on / off | `{ v: string \| number; tag: "c"; }` |
///
/// and every `c(k)` is `<T>(x: T) => { v: T; tag: "c"; }`.
#[test]
fn a_generic_chain_across_201_modules_answers_without_a_refusal() {
    let (outcome, _) = on_stack(8 << 20, || module_chain_work(201));
    assert_tagged_value(&outcome, &[number(), string()]);
}

/// The witness of a generic module chain `levels` modules long, before and
/// after its middle module `m(levels / 2)` is edited to wrap its callee's
/// return: `{ inner: c(k-1)(x), depth: k as const }`.
fn module_chain_across_a_middle_edit(levels: usize) -> (Outcome, Outcome) {
    let files = module_chain(levels, true);
    let sources: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let host = host_with(&sources);
    let entry = files.last().expect("a chain has a last module").0.clone();
    let witness = |host: &Arc<VerterHost>| {
        with_dispatch(host, |dispatch| {
            eval_key_on(host, dispatch, key_of(dispatch, &entry, "witness"))
        })
    };
    let before = witness(&host);
    let level = levels / 2;
    let previous = level - 1;
    let middle = &files[level].0;
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(middle.clone()),
        input_id: middle.clone(),
        source: Arc::from(format!(
            "import {{ c{previous} }} from \"./m{previous}\";\n\
             export function c{level}<T>(x: T) {{ return {{ inner: c{previous}(x), depth: {level} as const }}; }}\n"
        )),
        file_language: lang(middle),
        aliases: Vec::new(),
    });
    (before, witness(&host))
}

/// An edit to a module in the middle of a warm generic chain reaches the
/// witness at its top: every instantiation read off an uninstantiated
/// return carries that return's reads, and the functions above the edit —
/// whose warm answers no longer validate — are re-evaluated bottom-up by
/// the callee schedule, not one nested demand per invalidated level:
/// counting a stale candidate as answered, the edited chain answers at 16
/// modules and misses at 64, 128 and 201.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings alike, over the same 201 modules): with
/// `m100` edited to
/// `export function c100<T>(x: T) { return { inner: c99(x), depth: 100 as const }; }`,
/// `witness` is `{ inner: { v: string | number; tag: "c"; }; depth: 100; }`.
#[test]
fn an_edit_in_the_middle_of_a_module_chain_reaches_the_top() {
    let (before, after) = on_stack(8 << 20, || module_chain_across_a_middle_edit(201));
    assert_tagged_value(&before, &[number(), string()]);
    // Admitted beside the pre-edit candidate, which no longer validates.
    let Outcome::Value {
        ty: TypeExpr::Object(object),
        degradation: None,
        candidates: 2,
    } = &after
    else {
        panic!("expected a clean, admitted object after the edit: {after:?}");
    };
    let property = |name: &str| {
        object.properties.iter().find_map(|member| match member {
            ObjectMember::Property(property) if property.key == name.into() => Some(&property.ty),
            _ => None,
        })
    };
    assert_eq!(object.properties.len(), 2, "{after:?}");
    assert_eq!(
        property("depth"),
        Some(&TypeExpr::Literal(LiteralValue::Number(100.0))),
        "{after:?}"
    );
    let inner = property("inner").unwrap_or_else(|| panic!("no `inner` member: {after:?}"));
    assert_tagged_value(
        &Outcome::Value {
            ty: inner.clone(),
            degradation: None,
            candidates: 1,
        },
        &[number(), string()],
    );
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
    warm_second(&source, scheduled)
}

/// [`warm_local_chain_second`] over any `source` that exports `witness`
/// and `second`.
fn warm_second(source: &str, scheduled: bool) -> WarmSecond {
    let host = host_with(&[(PATH, source)]);
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

/// The witness of a same-file chain of `levels` non-generic direct calls,
/// evaluated with the callee schedule off, so every level's callee is
/// evaluated where the call sits: whether it answered, whether it is
/// partial and never admitted, its partial rails, and how many warm
/// candidates it admitted.
fn unscheduled_plain_chain(levels: usize) -> (bool, bool, PartialReasonSet, usize) {
    on_stack(8 << 20, move || {
        let _recursive = disable_flow_return_schedule_for_tests();
        let host = host_with(&[(PATH, plain_chain(levels).as_str())]);
        with_dispatch(&host, |dispatch| {
            let key = key_of(dispatch, PATH, "witness");
            let read = dispatch
                .execute_via_cold_build_helper(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
            let candidates = dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
            (
                matches!(read.value, QueryResult::Value(_)),
                read.result_is_partial && read.cache_suppress,
                read.partial_reasons,
                candidates,
            )
        })
    })
}

/// Native recursion the schedule does not predict ends in the typed depth
/// refusal, never in a stack overflow: an inline flow evaluation that would
/// nest past the connected demand's depth bound is refused with
/// `CONNECTED_QUERY_DEPTH_LIMIT`, partial and never admitted.
///
/// The witness is a same-file chain of direct calls evaluated with the
/// schedule off: each level nests its callee's evaluation natively, with no
/// query boundary between them. Below the bound (16 levels) it answers;
/// past it (256 levels, which without the bound overflows the 8 MiB
/// production worker stack in an unoptimized build) it is refused, typed.
///
/// Oracle (the pinned TypeScript 7.0.2, `--strict`): `witness` is
/// `{ v: number; tag: "c"; }` at both lengths (see
/// [`chains_through_other_call_shapes_consume_no_depth_per_level`]), so the
/// refusal is a typed gap, not an answer.
#[test]
fn an_unpredicted_deep_chain_ends_in_the_typed_depth_refusal() {
    let (answered, partial, reasons, candidates) = unscheduled_plain_chain(16);
    assert!(
        answered && !partial && candidates == 1,
        "a 16-level chain stays within the bound and answers: {reasons:?}"
    );
    let (answered, partial, reasons, candidates) = unscheduled_plain_chain(256);
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

/// A call nested inside another call's argument, and a member read there,
/// is evaluated in the frame it is written in: its callee, its own
/// arguments and the bindings they read are the frame's, and its value
/// takes the frame's carriers.
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
/// | `m1(o: { a: string })` = `id(o.a)` | `string` |
/// | `m2()` = `const box = { a: 1 }; return id(box.a);` | `number` |
/// | `m3(o: { a: { b: "k" } })` = `id(o.a.b)` | `"k"` |
///
/// Evaluated in the file's owner scope instead, the frame's `x`, `v`, `n`,
/// `s`, `o` and `box` are unbound, so each answered a semantic miss.
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
export function w7(n?: number) { return id(num(n ?? 1)); }\n\
export function m1(o: { a: string }) { return id(o.a); }\n\
export function m2() { const box = { a: 1 }; return id(box.a); }\n\
export function m3(o: { a: { b: \"k\" } }) { return id(o.a.b); }\n";
    for (name, expected) in [
        ("m1", "string"),
        ("m2", "number"),
        ("m3", "\"k\""),
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

/// The type-position chain's connected work is linear in its length: one
/// more level costs the same at 16, 64 and 200 levels.
///
/// Each level's `ReturnType<typeof t(N-1)>` resolves where the level
/// builds it, to the return below, as the checker resolves a conditional
/// type whose check type is not generic: no level publishes a carrier
/// over the level beneath it, so no reader projects the chain again.
///
/// Oracle: as for
/// [`a_200_level_type_position_chain_answers_on_the_default_stack`], at
/// every length.
#[test]
fn a_type_position_chain_costs_the_same_work_per_level() {
    let per_level = work_per_level(
        type_position_chain,
        "{ v: number; tag: \"c\"; }",
        &[16, 64, 200],
    );
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}

/// A 1,000-level type-position chain answers, under the production work
/// budget and on the default test stack.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings, measured at 200 levels — every level is the
/// same declaration): `witness` is `{ v: number; tag: "c"; }`.
#[test]
fn a_1000_level_type_position_chain_answers() {
    assert_answers_as_the_checker(&cold_read_of(
        &type_position_chain(1_000),
        "witness",
        "{ v: number; tag: \"c\"; }",
    ));
}

/// A body's `ReturnType<…>` resolves where it is built exactly where the
/// checker resolves it — its check type is not generic — and stays the
/// deferred application where the checker defers it; the published
/// return is the resolved type, as the checker prints it.
///
/// Oracle (the pinned TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`, identical under all four `strictNullChecks` x
/// `noImplicitAny` settings), over `t0(x: number)` and `g0<T>(x: T)`
/// returning `{ v: x, tag: "c" as const }`:
///
/// | function | declared return |
/// |---|---|
/// | `t1(x: number) { let r!: ReturnType<typeof t0>; return r; }` | `{ v: number; tag: "c"; }` |
/// | `t2(x: number) { let r!: ReturnType<typeof t1>; return r; }` | `{ v: number; tag: "c"; }` |
/// | `g1(x: number) { let r!: ReturnType<typeof g0>; return r; }` | `{ v: unknown; tag: "c"; }` |
/// | `d1<T>(x: T) { let r!: ReturnType<() => T>; return r; }` | `T` |
/// | `d3<T extends (...a: any) => any>(f: T) { let r!: ReturnType<T>; return r; }` | `ReturnType<T>` |
#[test]
fn a_return_type_in_a_body_resolves_unless_its_check_type_is_generic() {
    const SOURCE: &str = "\
function t0(x: number) { return { v: x, tag: \"c\" as const }; }\n\
export function t1(x: number) { let r!: ReturnType<typeof t0>; return r; }\n\
export function t2(x: number) { let r!: ReturnType<typeof t1>; return r; }\n\
function g0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n\
export function g1(x: number) { let r!: ReturnType<typeof g0>; return r; }\n\
export function d1<T>(x: T) { let r!: ReturnType<() => T>; return r; }\n\
export function d3<T extends (...a: any) => any>(f: T) { let r!: ReturnType<T>; return r; }\n";
    let host = host_with(&[(PATH, SOURCE)]);
    for name in ["t1", "t2"] {
        assert_tagged_value(&eval(&host, PATH, name), &[number()]);
    }
    assert_tagged_value(
        &eval(&host, PATH, "g1"),
        &[TypeExpr::Primitive(PrimitiveName::Unknown)],
    );
    assert_eq!(
        eval(&host, PATH, "d1"),
        Outcome::Value {
            ty: type_param("T"),
            degradation: None,
            candidates: 1,
        }
    );
    match &eval(&host, PATH, "d3") {
        Outcome::Value {
            ty: TypeExpr::Ref {
                name,
                type_arguments,
            },
            degradation: None,
            candidates: 1,
        } if name.as_ref() == "ReturnType" => {
            assert!(
                matches!(type_arguments.as_ref(), [TypeExpr::TypeParameter(param)] if param.name == "T"),
                "{type_arguments:?}"
            );
        }
        other => panic!("`d3` keeps the deferred `ReturnType<T>`: {other:?}"),
    }
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

/// A chain whose every level calls the next inside another call's
/// argument, inside a local arrow function:
/// `aN<T>(x: T) { const f = (y: T) => id(a(N-1)(y)); return f(x); }`.
fn arrow_nested_argument_chain(levels: usize) -> String {
    let mut source = "function id<T>(x: T) { return x; }\n\
                      function a0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n"
        .to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function a{level}<T>(x: T) {{ const f = (y: T) => id(a{}(y)); return f(x); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return a{}(v); }}\n",
        levels - 1
    ));
    source
}

/// A 200-level chain whose every edge is a call inside a generic call's
/// argument, inside a local arrow function, answers on the default test
/// stack: the callee return no discovery predicts is recorded by the
/// probe that meets it and evaluated first, from the explicit stack.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings): `witness` is `{ v: string | number; tag:
/// "c"; }` over the 200-level chain.
///
/// Beside the read of an instantiated callee off its uninstantiated
/// return, the chain's uninstantiated levels are evaluated inline, each
/// beneath the one above it (measured: a 6-level chain nests five such
/// evaluations): from 16 levels the chain is refused on the depth rail,
/// and at 200 levels an unoptimized build overflows the default test
/// stack, aborting the test process. With that read disabled the chain
/// answers at 71 units of connected work per level. Run this test alone.
#[test]
#[ignore = "a nested argument chain through local arrows answers without nesting a native evaluation per level"]
fn a_200_level_nested_argument_chain_in_local_arrows_answers_on_the_default_stack() {
    assert_answers_as_the_checker(&cold_read_of(
        &arrow_nested_argument_chain(200),
        "witness",
        "{ v: string | number; tag: \"c\"; }",
    ));
}

/// The nested-argument chain through local arrows costs the same work per
/// level at 16, 64 and 200 levels.
///
/// Oracle: as for
/// [`a_200_level_nested_argument_chain_in_local_arrows_answers_on_the_default_stack`],
/// at every length. The chain nests natively instead (see that test) and
/// overflows the default test stack from 16 levels in an unoptimized
/// build, aborting the test process. Run this test alone.
#[test]
#[ignore = "a nested argument chain through local arrows answers at the same work per level"]
fn a_nested_argument_chain_in_local_arrows_costs_the_same_work_per_level() {
    let per_level = work_per_level(
        arrow_nested_argument_chain,
        "{ v: string | number; tag: \"c\"; }",
        &[16, 64, 200],
    );
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}

/// A chain whose every level passes the next a member read of a local:
/// `kN<T>(x: T) { const box = { a: x }; return k(N-1)(box.a); }`.
fn member_read_chain(levels: usize) -> String {
    let mut source = "function k0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function k{level}<T>(x: T) {{ const box = {{ a: x }}; return k{}(box.a); }}\n",
            level - 1
        ));
    }
    let last = levels - 1;
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return k{last}(v); }}\n\
         export function second(v: boolean) {{ return k{last}(v); }}\n"
    ));
    source
}

/// A chain whose every level passes the next a member read of a
/// parameter beside the parameter:
/// `qN<T>(x: T, o: { a: T }) { return q(N-1)(o.a, o); }`.
fn parameter_member_chain(levels: usize) -> String {
    let mut source =
        "function q0<T>(x: T, o: { a: T }) { return { v: x, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function q{level}<T>(x: T, o: {{ a: T }}) {{ return q{}(o.a, o); }}\n",
            level - 1
        ));
    }
    let last = levels - 1;
    source.push_str(&format!(
        "export function witness(v: number | string, o: {{ a: number | string }}) {{ \
         return q{last}(v, o); }}\n\
         export function second(v: boolean, o: {{ a: boolean }}) {{ return q{last}(v, o); }}\n"
    ));
    source
}

/// A chain whose every level passes the next a literal beside its own
/// parameter: `rN<T, U>(x: T, u: U) { return r(N-1)(x, "lit"); }`.
fn literal_argument_chain(levels: usize) -> String {
    let mut source =
        "function r0<T, U>(x: T, u: U) { return { v: x, u, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function r{level}<T, U>(x: T, u: U) {{ return r{}(x, \"lit\"); }}\n",
            level - 1
        ));
    }
    let last = levels - 1;
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return r{last}(v, 0); }}\n\
         export function second(v: boolean) {{ return r{last}(v, 0); }}\n"
    ));
    source
}

/// A chain whose every level passes the next a call on its parameter:
/// `sN<T>(x: T) { return s(N-1)(id(x)); }`.
fn call_argument_chain(levels: usize) -> String {
    let mut source = "function id<T>(x: T) { return x; }\n\
                      function s0<T>(x: T) { return { v: x, tag: \"c\" as const }; }\n"
        .to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function s{level}<T>(x: T) {{ return s{}(id(x)); }}\n",
            level - 1
        ));
    }
    let last = levels - 1;
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return s{last}(v); }}\n\
         export function second(v: boolean) {{ return s{last}(v); }}\n"
    ));
    source
}

/// A first request's `witness` over a chain `source`, then a second
/// request's `second` over the warm chain, each answers the checker's
/// print, clean and admitted.
#[track_caller]
fn assert_warm_second_answers(source: &str, witness: &str, second: &str) {
    let host = host_with(&[(PATH, source)]);
    for (name, expected) in [("witness", witness), ("second", second)] {
        let read = with_dispatch(&host, |dispatch| cold_read(dispatch, PATH, name, expected));
        assert_answers_as_the_checker(&read);
    }
}

/// The connected work of the second request's `second` over a warm chain
/// of `levels`.
fn warm_second_work(chain: ChainSource, levels: usize, second: &str) -> usize {
    let host = host_with(&[(PATH, chain(levels).as_str())]);
    with_dispatch(&host, |dispatch| {
        let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key_of(
            dispatch, PATH, "witness",
        ))));
    });
    with_dispatch(&host, |dispatch| {
        let read = cold_read(dispatch, PATH, "second", second);
        assert_answers_as_the_checker(&read);
        read.work
    })
}

/// A new instantiation of a warm 200-level `chain` answers on the default
/// test stack, and one more level costs the same work at 16, 32 and 64
/// levels.
#[track_caller]
fn assert_warm_argument_form(chain: ChainSource, witness: &str, second: &str) {
    assert_warm_second_answers(&chain(200), witness, second);
    let per_level: Vec<usize> = [16, 32, 64]
        .into_iter()
        .map(|levels| {
            warm_second_work(chain, levels + 1, second) - warm_second_work(chain, levels, second)
        })
        .collect();
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}

// A new instantiation of a warm chain answers on the default test stack
// whatever each level passes the next: the instantiation each level
// demands is the call executor's own, recorded by the probe that meets it
// and evaluated first, from the explicit stack.
//
// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
// `noImplicitAny` settings, over each 200-level chain):
//
// | chain | `witness(v: number \| string, …)` | `second(v: boolean, …)` |
// |---|---|---|
// | parameter member read `q(N-1)(o.a, o)` | `{ v: string \| number; tag: "c"; }` | `{ v: boolean; tag: "c"; }` |
// | local member read `k(N-1)(box.a)` | `{ v: string \| number; tag: "c"; }` | `{ v: boolean; tag: "c"; }` |
// | literal `r(N-1)(x, "lit")` | `{ v: string \| number; u: string; tag: "c"; }` | `{ v: boolean; u: string; tag: "c"; }` |
// | call `s(N-1)(id(x))` | `{ v: string \| number; tag: "c"; }` | `{ v: boolean; tag: "c"; }` |

/// A parameter member read (`q(N-1)(o.a, o)`); oracle in the table above.
#[test]
fn a_warm_chain_passing_a_parameter_member_read_instantiates_stacklessly() {
    assert_warm_argument_form(
        parameter_member_chain,
        "{ v: string | number; tag: \"c\"; }",
        "{ v: boolean; tag: \"c\"; }",
    );
}

/// A local member read (`k(N-1)(box.a)`); oracle in the table above.
#[test]
fn a_warm_chain_passing_a_local_member_read_instantiates_stacklessly() {
    assert_warm_argument_form(
        member_read_chain,
        "{ v: string | number; tag: \"c\"; }",
        "{ v: boolean; tag: \"c\"; }",
    );
}

/// A literal beside the parameter (`r(N-1)(x, "lit")`); oracle in the
/// table above.
#[test]
fn a_warm_chain_passing_a_literal_instantiates_stacklessly() {
    assert_warm_argument_form(
        literal_argument_chain,
        "{ v: string | number; u: string; tag: \"c\"; }",
        "{ v: boolean; u: string; tag: \"c\"; }",
    );
}

/// A call on the parameter (`s(N-1)(id(x))`); oracle in the table above.
#[test]
fn a_warm_chain_passing_a_call_on_its_parameter_instantiates_stacklessly() {
    assert_warm_argument_form(
        call_argument_chain,
        "{ v: string | number; tag: \"c\"; }",
        "{ v: boolean; tag: \"c\"; }",
    );
}

/// A chain whose every level passes the next an object literal holding a
/// member read of its parameter:
/// `bN<T>(o: { a: T }) { return b(N-1)({ a: o.a }); }`.
fn object_literal_argument_chain(levels: usize) -> String {
    let mut source =
        "function b0<T>(o: { a: T }) { return { v: o.a, tag: \"c\" as const }; }\n".to_string();
    for level in 1..levels {
        source.push_str(&format!(
            "function b{level}<T>(o: {{ a: T }}) {{ return b{}({{ a: o.a }}); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function witness(v: number | string) {{ return b{}({{ a: v }}); }}\n",
        levels - 1
    ));
    source
}

/// Native recursion through callees the schedule does not evaluate ends in
/// the typed depth refusal on the production worker stack: with the
/// schedule off every level of the object-literal chain nests, and the
/// connected-query depth guard refuses the chain from 12 levels, partial
/// and never admitted, however long it is.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings, over the 200-level chain): `witness` is
/// `{ v: string | number; tag: "c"; }`, so the refusal is a typed gap, not
/// an answer; with the schedule on the chain answers it
/// (`a_chain_of_degraded_callees_answers_on_the_default_stack`). The 8 MiB
/// thread is the production worker stack the bound is sized for.
#[test]
fn a_chain_of_degraded_callees_ends_in_the_typed_refusal_on_the_worker_stack() {
    let read = on_stack(8 << 20, || {
        let _recursive = disable_flow_return_schedule_for_tests();
        cold_read_of(
            &object_literal_argument_chain(200),
            "witness",
            "{ v: string | number; tag: \"c\"; }",
        )
    });
    assert!(
        read.partial && read.candidates == 0,
        "refused partial and never admitted: {:?}",
        read.reasons
    );
    assert!(
        read.reasons
            .contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT),
        "the refusal is the depth rail's: {:?}",
        read.reasons
    );
}

/// An object or array literal written as a call argument is evaluated in
/// the frame it is written in, so the bindings it reads are the frame's.
///
/// Oracle (the pinned TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`, identical under all four `strictNullChecks` x
/// `noImplicitAny` settings), over `id<T>(x: T)`, `idc<const T>(x: T)`,
/// `box<T>(o: { a: T })` returning `o.a` and `first<T>(xs: T[])` returning
/// `xs[0]`:
///
/// | function | declared return |
/// |---|---|
/// | `o1(v: number)` = `id({ a: v })` | `{ a: number; }` |
/// | `o2(v: number)` = `id({ a: v, s: "lit" })` | `{ a: number; s: string; }` |
/// | `o3(o: { a: string })` = `id({ a: o.a })` | `{ a: string; }` |
/// | `o4(v: number)` = `box({ a: v })` | `number` |
/// | `a1(v: number, o: { a: string })` = `id([v, o.a])` | `(string \| number)[]` |
/// | `a2(v: number)` = `first([v, 1])` | `number` |
/// | `c1(v: number)` = `idc({ a: v, s: "lit" })` | `{ readonly a: number; readonly s: "lit"; }` |
/// | `c2(v: number)` = `idc([v, "lit"])` | `readonly [number, "lit"]` |
///
/// The literal is a frame value, so `o1`..`a2` answer the checker's type.
/// A `const` type parameter's argument reads its const context,
/// [`a_const_type_parameter_reads_a_frame_literal_argument_in_its_const_context`].
#[test]
fn an_object_or_array_literal_argument_evaluates_in_its_own_frame() {
    for (name, expected) in [
        ("o1", "{ a: number; }"),
        ("o2", "{ a: number; s: string; }"),
        ("o3", "{ a: string; }"),
        ("o4", "number"),
        ("a1", "(string | number)[]"),
        ("a2", "number"),
    ] {
        assert_answers_as_the_checker(&cold_read_of(LITERAL_ARGUMENT_SOURCE, name, expected));
    }
}

/// The functions of [`an_object_or_array_literal_argument_evaluates_in_its_own_frame`].
const LITERAL_ARGUMENT_SOURCE: &str = "\
function id<T>(x: T) { return x; }\n\
function idc<const T>(x: T) { return x; }\n\
function box<T>(o: { a: T }) { return o.a; }\n\
function first<T>(xs: T[]) { return xs[0]; }\n\
export function o1(v: number) { return id({ a: v }); }\n\
export function o2(v: number) { return id({ a: v, s: \"lit\" }); }\n\
export function o3(o: { a: string }) { return id({ a: o.a }); }\n\
export function o4(v: number) { return box({ a: v }); }\n\
export function a1(v: number, o: { a: string }) { return id([v, o.a]); }\n\
export function a2(v: number) { return first([v, 1]); }\n\
export function c1(v: number) { return idc({ a: v, s: \"lit\" }); }\n\
export function c2(v: number) { return idc([v, \"lit\"]); }\n\
export function c3(v: number) { return idc({ a: v, n: { s: \"lit\" }, xs: [v, 1] }); }\n\
function boxc<const T>(o: { a: T }) { return o.a; }\n\
export function c4(v: number) { return boxc({ a: [v, \"lit\"] }); }\n";

/// A literal argument of a `const` type parameter that reads the frame is
/// read in its const context (`isConstContext`): literals kept, members
/// and nested tuples readonly.
///
/// Measured on TypeScript 7.0.2 (all four settings alike): `c1` is `{
/// readonly a: number; readonly s: "lit"; }`, `c2` `readonly [number,
/// "lit"]`, and `c3` `{ readonly a: number; readonly n: { readonly s:
/// "lit"; }; readonly xs: readonly [number, 1]; }`.
#[test]
fn a_const_type_parameter_reads_a_frame_literal_argument_in_its_const_context() {
    for (name, expected) in [
        ("c1", "{ readonly a: number; readonly s: \"lit\"; }"),
        ("c2", "readonly [number, \"lit\"]"),
        (
            "c3",
            "{ readonly a: number; readonly n: { readonly s: \"lit\"; }; readonly xs: readonly [number, 1]; }",
        ),
    ] {
        assert_answers_as_the_checker(&cold_read_of(LITERAL_ARGUMENT_SOURCE, name, expected));
    }
}

/// A literal member whose contextual type is a `const` type parameter
/// nested in the parameter's type is read in its const context.
///
/// Measured on TypeScript 7.0.2 (all four settings alike): `c4`
/// (`boxc<const T>(o: { a: T })` over `{ a: [v, "lit"] }`) is `readonly
/// [number, "lit"]`. The lane reads the member widened and answers
/// `readonly (string | number)[]`.
#[test]
#[ignore = "a literal member read in the const context of a const type parameter nested in its parameter's type"]
fn a_const_type_parameter_nested_in_the_parameter_reads_its_member_in_its_const_context() {
    assert_answers_as_the_checker(&cold_read_of(
        LITERAL_ARGUMENT_SOURCE,
        "c4",
        "readonly [number, \"lit\"]",
    ));
}

/// A 200-level chain whose every level passes the next an object literal
/// answers on the default test stack.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings): `witness` is `{ v: string | number; tag:
/// "c"; }` over the 200-level chain.
///
/// The literal argument is a frame value evaluated in the frame it is
/// written in, so every level answers and is reusable, and the schedule
/// evaluates the chain bottom-up without nesting a native evaluation per
/// level.
#[test]
fn a_chain_of_degraded_callees_answers_on_the_default_stack() {
    assert_answers_as_the_checker(&cold_read_of(
        &object_literal_argument_chain(200),
        "witness",
        "{ v: string | number; tag: \"c\"; }",
    ));
}

/// The object-literal chain costs the same work per level at 16, 64 and
/// 200 levels.
///
/// Oracle: as for `a_chain_of_degraded_callees_answers_on_the_default_stack`,
/// at every length.
#[test]
fn an_object_literal_argument_chain_costs_the_same_work_per_level() {
    let per_level = on_stack(8 << 20, || {
        work_per_level(
            object_literal_argument_chain,
            "{ v: string | number; tag: \"c\"; }",
            &[16, 64, 200],
        )
    });
    assert!(
        per_level.windows(2).all(|pair| pair[0] == pair[1]),
        "work per level: {per_level:?}"
    );
}

/// A new instantiation of a warm chain through local arrow functions is
/// read off the chain's uninstantiated return: no level is evaluated again
/// under the new instantiation, so the 256-level chain answers, clean and
/// admitted, on a 512 KiB stack.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings alike, measured at 256 levels):
/// `second(v: boolean)` is `{ v: boolean; tag: "c"; }`.
#[test]
fn a_new_instantiation_of_a_warm_local_arrow_chain_is_read_off_its_uninstantiated_return() {
    let mut source = local_arrow_chain(256);
    source.push_str("export function second(v: boolean) { return l255(v); }\n");
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

/// Chains through local arrow functions whose head returns a nested
/// generic declaration re-declaring the chain's binder name — a function
/// value (`id: <T,>(z: T) => z`) or a class expression
/// (`K: class<T> { own!: T; }`): the nested clause is its own declaration,
/// with binders distinct from every level's `T`, so each level's
/// instantiation is read off its uninstantiated return without touching
/// the nested parameter. Each 256-level chain answers, and a new
/// instantiation of the warm chain answers on a 512 KiB stack.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings alike, measured at 256 levels): `witness` is
/// `{ v: string | number; tag: "c"; id: <T>(z: T) => T; }` and
/// `second(v: boolean)` is `{ v: boolean; tag: "c"; id: <T>(z: T) => T; }`
/// over the function head; over the class head the `id` member is
/// `K: { new <T>(): { own: T; }; }`.
#[test]
fn a_256_level_chain_returning_a_same_name_generic_answers() {
    for (head, member, keeps_its_clause) in [
        (
            "id: <T,>(z: T) => z",
            "id",
            identity_of_its_own_t as fn(&TypeExpr) -> bool,
        ),
        ("K: class<T> { own!: T; }", "K", constructor_of_its_own_t),
    ] {
        let source = local_arrow_chain(256).replacen(
            "{ v: x, tag: \"c\" as const }",
            &format!("{{ v: x, tag: \"c\" as const, {head} }}"),
            1,
        );
        assert_same_name_generic_chain_answers(source, member, keeps_its_clause, 512 << 10);
    }
}

/// The same chain over a head holding a function TYPE written in the body
/// with a same-name clause (`const id: <T>(z: T) => T = (z) => z`): a new
/// instantiation of the 256-level chain answers too.
///
/// Oracle (the pinned TypeScript 7.0.2, all four `strictNullChecks` x
/// `noImplicitAny` settings alike, measured at 256 levels): `witness` is
/// `{ v: string | number; tag: "c"; id: <T>(z: T) => T; }` and
/// `second(v: boolean)` is `{ v: boolean; tag: "c"; id: <T>(z: T) => T; }`.
#[test]
fn a_256_level_chain_returning_a_same_name_function_type_answers() {
    let source = local_arrow_chain(256).replacen(
        "function l0<T>(x: T) { return { v: x, tag: \"c\" as const }; }",
        "function l0<T>(x: T) { const id: <T>(z: T) => T = (z) => z; \
         return { v: x, tag: \"c\" as const, id }; }",
        1,
    );
    // The production worker stack: the per-level evaluation meets the
    // typed depth refusal rather than a small test stack's end.
    assert_same_name_generic_chain_answers(source, "id", identity_of_its_own_t, 8 << 20);
}

/// A binder named `T`.
fn is_t(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::TypeParameter(param) if param.name == "T")
}

/// `<T>(z: T) => T`.
fn identity_of_its_own_t(member: &TypeExpr) -> bool {
    matches!(member, TypeExpr::Function(id)
        if id.type_parameters.len() == 1
            && id.type_parameters[0].name == "T"
            && id.parameters.len() == 1
            && is_t(&id.parameters[0].ty)
            && id.return_type.as_deref().is_some_and(is_t))
}

/// `{ new <T>(): { own: T; }; }`.
fn constructor_of_its_own_t(member: &TypeExpr) -> bool {
    let TypeExpr::Object(constructor) = member else {
        return false;
    };
    // Beside its construct signature a class's constructor type carries
    // its `prototype` property.
    let signatures: Vec<&ObjectMember> = constructor
        .properties
        .iter()
        .filter(|member| {
            !matches!(member, ObjectMember::Property(property) if property.key == "prototype".into())
        })
        .collect();
    let [ObjectMember::ConstructSignature(construct)] = signatures.as_slice() else {
        return false;
    };
    let Some(TypeExpr::Object(instance)) = construct.return_type.as_deref() else {
        return false;
    };
    construct.type_parameters.len() == 1
        && construct.type_parameters[0].name == "T"
        && matches!(instance.properties.as_slice(),
            [ObjectMember::Property(own)] if own.key == "own".into() && is_t(&own.ty))
}

/// Over a 256-level chain `source` ending in `l255`: `witness` answers
/// `{ v: string | number; tag: "c"; <member> }` cold, and a new
/// instantiation `second(v: boolean)` answers `{ v: boolean; tag: "c";
/// <member> }` on a `second_stack`-byte stack, clean and admitted, with
/// `member` keeping its own clause.
fn assert_same_name_generic_chain_answers(
    mut source: String,
    member: &str,
    keeps_its_clause: fn(&TypeExpr) -> bool,
    second_stack: usize,
) {
    source.push_str("export function second(v: boolean) { return l255(v); }\n");
    let host = host_with(&[(PATH, source.as_str())]);
    let witness = {
        let host = Arc::clone(&host);
        on_stack(8 << 20, move || {
            with_dispatch(&host, |dispatch| {
                eval_key_on(&host, dispatch, key_of(dispatch, PATH, "witness"))
            })
        })
    };
    let second = on_stack(second_stack, move || {
        with_dispatch(&host, |dispatch| {
            eval_key_on(&host, dispatch, key_of(dispatch, PATH, "second"))
        })
    });
    for (name, outcome, arms) in [
        ("witness", &witness, vec![number(), string()]),
        (
            "second",
            &second,
            vec![TypeExpr::Primitive(PrimitiveName::Boolean)],
        ),
    ] {
        let Outcome::Value {
            ty: TypeExpr::Object(object),
            degradation: None,
            candidates: 1,
        } = outcome
        else {
            panic!("expected a clean, admitted object for `{name}`: {outcome:?}");
        };
        let property = |name: &str| {
            object.properties.iter().find_map(|entry| match entry {
                ObjectMember::Property(property) if property.key == name.into() => {
                    Some(&property.ty)
                }
                _ => None,
            })
        };
        assert_eq!(object.properties.len(), 3, "{outcome:?}");
        assert_eq!(
            property("tag"),
            Some(&TypeExpr::Literal(LiteralValue::String("c".to_string()))),
            "{outcome:?}"
        );
        match property("v") {
            Some(TypeExpr::Union(members)) => assert!(
                members.len() == arms.len() && arms.iter().all(|arm| members.contains(arm)),
                "{outcome:?}"
            ),
            Some(single) => assert_eq!(std::slice::from_ref(single), &arms[..], "{outcome:?}"),
            None => panic!("no `v` member: {outcome:?}"),
        }
        assert!(
            property(member).is_some_and(keeps_its_clause),
            "`{member}` keeps its own `<T>`: {outcome:?}"
        );
    }
}
