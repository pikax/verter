//! A chain of guards over one union subject — `instanceof`, `in`, a
//! user type predicate, a discriminant equality — narrows at a cost that
//! grows with the chain, not with the chain times the arms each guard
//! passes over:
//!
//! - every guard enumerates the subject's remaining arms, as the checker's
//!   `filterType` does, but an arm's carrier unwrap, its tested member and
//!   its relation to a predicate's target are read once per evaluation,
//!   never once per guard;
//! - an `instanceof` guard decides by heritage and pays no relation for
//!   the arms it passes over once an arm survives by derivation;
//! - a discriminant test past a negated one answers the arms whose tag is
//!   one of the remaining tags without a relation per remaining tag.
//!
//! Each demand is the named demand the semantic benchmark makes
//! (`__Probe = ReturnType<typeof f>` resolved by name and projected to its
//! wire answer). Every expected answer is TypeScript 7.0.2's for the same
//! program under `--strict`: each guard's return value, and `undefined`
//! for the end of the body, which stays reachable to the checker.
//!
//! The growth proofs compare three sizes, each double the one before. A
//! user type predicate must relate every remaining arm to its target at
//! every guard, as the checker's `getNarrowedType` does, so that chain's
//! relation count is quadratic by definition; its proof bounds the count
//! to one relation per guard and remaining arm, and its cold builds
//! linearly.

use std::sync::Arc;

use verter_type_engine::project_semantic_dispatch::flow_return::guard_applications_for_tests;
use verter_type_engine::project_semantic_dispatch::raise::{
    enable_dispatch_trace_for_test, DISPATCH_TRACE,
};
use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

const SCENARIO: &str = "/narrowing/scenario.ts";

/// `classes` distinct classes, `C{i}` holding `k{i} = i`, and their union
/// `U`.
fn classes(classes: usize) -> String {
    let mut source: String = (0..classes)
        .map(|i| format!("class C{i} {{ k{i} = {i} as const; }}\n"))
        .collect();
    let union = (0..classes)
        .map(|i| format!("C{i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    source.push_str(&format!("type U = {union};\n"));
    source
}

/// `f(x: U)` returning `x.k{i}` under one guard per class, in class order.
fn guard_chain(arms: usize, guard: impl Fn(usize) -> String) -> String {
    let mut source = String::from("function f(x: U) {\n");
    for i in 0..arms {
        source.push_str(&format!("  if ({}) return x.k{i};\n", guard(i)));
    }
    source.push_str("}\n");
    source
}

fn instanceof_chain(arms: usize) -> String {
    classes(arms) + &guard_chain(arms, |i| format!("x instanceof C{i}"))
}

fn in_chain(arms: usize) -> String {
    classes(arms) + &guard_chain(arms, |i| format!("\"k{i}\" in x"))
}

fn predicate_chain(arms: usize) -> String {
    let predicates: String = (0..arms)
        .map(|i| format!("declare function is{i}(x: unknown): x is C{i};\n"))
        .collect();
    classes(arms) + &predicates + &guard_chain(arms, |i| format!("is{i}(x)"))
}

/// A typed-router narrowing: a route location per named route, read off a
/// route map through a mapped type, narrowed by its name. Every route but
/// the last returns its one string parameter; the last returns its whole
/// parameter object.
fn router_chain(routes: usize) -> String {
    let mut source = String::from("interface RouteNamedMap {\n");
    for i in 0..routes {
        source.push_str(&format!("  r{i}: {{ params: {{ id{i}: string }} }};\n"));
    }
    source.push_str(
        "}\n\
         type RouteLocation = { [K in keyof RouteNamedMap]: { name: K; params: RouteNamedMap[K][\"params\"] } }[keyof RouteNamedMap];\n\
         function f(route: RouteLocation) {\n",
    );
    let last = routes - 1;
    for i in 0..last {
        source.push_str(&format!(
            "  if (route.name === \"r{i}\") return route.params.id{i};\n"
        ));
    }
    source.push_str(&format!(
        "  if (route.name === \"r{last}\") return route.params;\n}}\n"
    ));
    source
}

/// The work one demand did, by the counters a growth proof compares.
#[derive(Debug, Clone, Copy)]
struct Work {
    connected_work: u64,
    query_reads: u64,
    cold_builds: u64,
    relation_checks: u64,
    guards: u64,
}

/// Resolve `__Probe = ReturnType<typeof f>` in a fresh strict project
/// holding `source`, as the benchmark's probe does, and return the wire
/// answer's arms and the demand's work.
fn chain_probe(source: &str) -> (Vec<TypeExpr>, Work) {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/narrowing", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!(
                "{source}type __Probe = ReturnType<typeof f>;\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let _trace = enable_dispatch_trace_for_test();
    let guards_before = guard_applications_for_tests();
    let ((outcome, _), usage) =
        verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(
            || {
                host.resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
                    .into_parts()
            },
        );
    let guards = (guard_applications_for_tests() - guards_before) as u64;
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let query_reads = DISPATCH_TRACE.with(|trace| trace.borrow().len()) as u64;
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let answer: TypeExpr = serde_json::from_slice(&bytes).expect("a wire answer");
    let TypeExpr::Union(arms) = &answer else {
        panic!("`{answer:?}` is not a union");
    };
    (
        arms.to_vec(),
        Work {
            connected_work: usage.work as u64,
            query_reads,
            cold_builds: stats.misses,
            relation_checks: stats.relation_check_count,
            guards,
        },
    )
}

/// The checker's answer to a class chain: every `k{i}` literal and the
/// end of the body's `undefined`.
fn assert_literals_and_undefined(label: &str, arms: &[TypeExpr], count: usize) {
    let mut literals = Vec::new();
    let mut undefined = 0;
    for arm in arms {
        match arm {
            TypeExpr::Literal(LiteralValue::Number(value)) if value.fract() == 0.0 => {
                literals.push(*value as usize)
            }
            TypeExpr::Primitive(PrimitiveName::Undefined) => undefined += 1,
            other => panic!("{label}: the answer holds {other:?}"),
        }
    }
    literals.sort_unstable();
    assert_eq!(
        literals,
        (0..count).collect::<Vec<_>>(),
        "{label}: the literals"
    );
    assert_eq!(undefined, 1, "{label}: the end of the body's `undefined`");
}

/// The checker's answer to the router chain:
/// `string | undefined | { id{last}: string }`.
fn assert_router_answer(label: &str, arms: &[TypeExpr], routes: usize) {
    let last = format!("id{}", routes - 1);
    let mut seen = (false, false, false);
    for arm in arms {
        match arm {
            TypeExpr::Primitive(PrimitiveName::String) => seen.0 = true,
            TypeExpr::Primitive(PrimitiveName::Undefined) => seen.1 = true,
            TypeExpr::Object(object) => {
                let [ObjectMember::Property(property)] = &object.properties[..] else {
                    panic!("{label}: the parameter object is {object:?}");
                };
                assert_eq!(
                    property.key.as_string(),
                    Some(last.as_str()),
                    "{label}: the parameter name"
                );
                assert_eq!(
                    property.ty,
                    TypeExpr::Primitive(PrimitiveName::String),
                    "{label}: the parameter type"
                );
                assert!(!property.optional, "{label}: the parameter is required");
                seen.2 = true;
            }
            other => panic!("{label}: the answer holds {other:?}"),
        }
    }
    assert_eq!(
        (arms.len(), seen),
        (3, (true, true, true)),
        "{label}: {arms:?}"
    );
}

/// Between a size and its double each counter may grow at most 1.5 times
/// faster than the input, so at most threefold; a quadratic term grows
/// fourfold. The counters must be live at every size, so a counter that
/// stopped counting cannot pass as linear.
fn assert_linear(label: &str, what: &str, measured: &[(usize, u64)]) {
    for (size, value) in measured {
        assert!(*value > 0, "{label}: {what} is zero at {size}");
    }
    for pair in measured.windows(2) {
        let [(small, before), (large, after)] = pair else {
            unreachable!();
        };
        assert!(
            2 * after * *small as u64 <= 3 * before * *large as u64,
            "{label}: {what} grew from {before} at {small} to {after} at {large}: {measured:?}"
        );
    }
}

/// The three sizes of a growth proof, each double the one before.
fn sizes(smallest: usize) -> [usize; 3] {
    [smallest, smallest * 2, smallest * 4]
}

/// Measure a chain at three sizes, asserting each answer, and prove its
/// reads, cold builds and connected work linear in the chain.
fn assert_chain_linear(
    label: &str,
    smallest: usize,
    chain: fn(usize) -> String,
    answer: fn(&str, &[TypeExpr], usize),
) -> Vec<(usize, Work)> {
    let measured: Vec<(usize, Work)> = sizes(smallest)
        .into_iter()
        .map(|size| {
            let (arms, work) = chain_probe(&chain(size));
            answer(&format!("{label} over {size}"), &arms, size);
            assert_eq!(
                work.guards,
                2 * size as u64,
                "{label} over {size}: each guard applies once per edge"
            );
            (size, work)
        })
        .collect();
    let series = |read: fn(&Work) -> u64| -> Vec<(usize, u64)> {
        measured
            .iter()
            .map(|(size, work)| (*size, read(work)))
            .collect()
    };
    assert_linear(label, "query reads", &series(|work| work.query_reads));
    assert_linear(label, "cold builds", &series(|work| work.cold_builds));
    assert_linear(label, "connected work", &series(|work| work.connected_work));
    measured
}

#[test]
fn instanceof_chain_narrows_in_linear_work() {
    let measured = assert_chain_linear(
        "instanceof",
        25,
        instanceof_chain,
        assert_literals_and_undefined,
    );
    // Heritage decides every guard: no arm needs the union-level
    // fallback's relations.
    for (size, work) in measured {
        assert_eq!(work.relation_checks, 0, "instanceof over {size}: {work:?}");
    }
}

#[test]
fn in_chain_narrows_in_linear_work() {
    assert_chain_linear("in", 25, in_chain, assert_literals_and_undefined);
}

#[test]
fn router_discriminant_chain_narrows_in_linear_work() {
    assert_chain_linear("router", 25, router_chain, assert_router_answer);
}

/// Every guard relates each remaining arm to its target once, its
/// negated edge reusing its positive edge's relations, and every class
/// is built once.
#[test]
fn predicate_chain_relates_each_arm_once_per_guard() {
    let measured: Vec<(usize, Work)> = sizes(15)
        .into_iter()
        .map(|size| {
            let (arms, work) = chain_probe(&predicate_chain(size));
            assert_literals_and_undefined(&format!("predicate over {size}"), &arms, size);
            let pairs = (size * (size - 1) / 2) as u64;
            assert!(
                work.relation_checks > 0 && work.relation_checks <= pairs,
                "predicate over {size}: {} relations for {pairs} guard and remaining-arm pairs",
                work.relation_checks
            );
            (size, work)
        })
        .collect();
    let cold: Vec<(usize, u64)> = measured
        .iter()
        .map(|(size, work)| (*size, work.cold_builds))
        .collect();
    assert_linear("predicate", "cold builds", &cold);
}
