//! A literal related to a union target that holds it answers from the
//! union's membership, as the checker's `containsType` test does, so
//! relating a wide literal union to its superset prepares the target once
//! and looks up each member by canonical value.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --noEmit --strict` under each of
//! `strictNullChecks` and `noImplicitAny` on and off (the four settings
//! agree on every probe) as `const s: "q" = null! as <probe>;`, read off
//! the TS2322 message.

use super::checker_probe_lane_tests::{mismatches, with_probe};

/// `type W = "m0" | "m1" | … `, `width` members.
fn wide_union(width: usize) -> String {
    let members: Vec<String> = (0..width).map(|index| format!("\"m{index}\"")).collect();
    format!("type W = {};\n", members.join(" | "))
}

const EXCLUDED_TO_WHOLE: &str = "[Exclude<W, \"m0\">] extends [W] ? 1 : 2";

/// Count physical arm reads or native Omit key operations: memoized relation
/// counts alone conceal a scan of the target for every source literal.
#[test]
fn literal_union_membership_work_grows_with_input() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    for probe in [
        "Exclude<W, Most>",
        "[Most] extends [W] ? 1 : 2",
        "keyof Omit<{ [K in W]: K }, Most>",
    ] {
        let count = if probe.starts_with("keyof Omit") {
            super::ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::omit_key_filter_steps_for_tests
                as fn() -> usize
        } else {
            super::ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_union_arm_visits_for_tests
                as fn() -> usize
        };
        let visits = [32, 64, 128].map(|width| {
            let most = (0..width)
                .filter(|index| ![3, 5].contains(index))
                .map(|index| format!("\"m{index}\""))
                .collect::<Vec<_>>()
                .join(" | ");
            let source = format!("{}type Most = {most};", wide_union(width));
            let before = count();
            with_probe(&source, probe, |dispatch, node| {
                let expected = checker_syntax::parse(if probe.starts_with('[') {
                    "1"
                } else {
                    "\"m3\" | \"m5\""
                })
                .expect("valid expected type");
                assert!(checker_syntax::matches_node(dispatch, node, &expected, 0));
            });
            count() - before
        });
        println!("literal membership {probe}: membership steps {visits:?}");
        assert!(
            visits.iter().all(|visits| *visits > 0),
            "membership work must be observed: {probe}: {visits:?}"
        );
        for step in 0..2 {
            assert!(
                visits[step + 1] * 10 <= visits[step] * 22,
                "doubling the input at most doubles membership work: {probe}: {visits:?}"
            );
        }
    }
}

/// The module declares its own `Exclude` (a distributive conditional, not
/// the native utility) and derives `Most` with it. Every distributed
/// member relates to the one `Most`, so reading what `Most` names, reducing
/// that union and classifying it as an `extends` operand (generic, `infer`
/// sites) happen once for the distribution, not once per member, whether
/// the probe is a value's declared type or a named alias the host
/// resolves. Omit over the declared utilities distributes the same way.
///
/// Oracle: TypeScript 7.0.2 answers `"m3" | "m5"` for both probes at every
/// width (`--strict`).
#[test]
fn a_declared_exclude_reads_a_derived_union_once_per_distribution() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};
    type Dispatch<'a> = super::ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    const DECLARED: &str = "type Exclude<T, U> = T extends U ? never : T;\n\
        type Pick<T, K extends keyof T> = { [P in K]: T[P] };\n\
        type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;\n";
    let reads = || {
        Dispatch::composite_arm_reads_for_tests()
            + Dispatch::relation_union_arm_visits_for_tests()
            + Dispatch::conditional_operand_walk_visits_for_tests()
    };
    // The string literals of the alias `__Probe` the host resolves, in order.
    let named_answer = |source: &str| {
        let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some("/scenario.ts".to_owned()),
                input_id: "/scenario.ts".to_owned(),
                source: Arc::from(source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert the scenario");
        let node = host
            .resolve_named_symbol("/scenario.ts", "__Probe", None)
            .expect("the probe answers");
        let graph = host.project_type_store().semantic_graph();
        let mut pending = vec![node.id()];
        let mut values = Vec::new();
        while let Some(node) = pending.pop() {
            match graph.node_data(node).as_deref().expect("live answer") {
                SemanticNodeData::Union(members) => pending.extend(members.iter().rev().copied()),
                SemanticNodeData::Alias(inner) => pending.push(*inner),
                SemanticNodeData::Literal(LiteralValue::String(value)) => {
                    values.push(value.to_string());
                }
                other => panic!("unexpected answer {other:?}"),
            }
        }
        values
    };
    for named in [false, true] {
        for probe in ["Exclude<W, Most>", "keyof Omit<{ [K in W]: K }, Most>"] {
            let visits = [32, 64, 128].map(|width| {
                let source = format!(
                    "{DECLARED}{}type Most = Exclude<W, \"m3\" | \"m5\">;\n",
                    wide_union(width)
                );
                let before = reads();
                if named {
                    let source = format!("{source}type __Probe = {probe};\nexport {{}};\n");
                    assert_eq!(named_answer(&source), ["m3", "m5"], "{probe} over {width}");
                } else {
                    with_probe(&source, probe, |dispatch, node| {
                        let expected =
                            checker_syntax::parse("\"m3\" | \"m5\"").expect("valid expected type");
                        assert!(
                            checker_syntax::matches_node(dispatch, node, &expected, 0),
                            "{probe} over {width} members"
                        );
                    });
                }
                reads() - before
            });
            println!("declared {probe} (named: {named}): union reads {visits:?}");
            // Preparing the membership index alone reads every arm once: a
            // count below the width means the work went uncounted.
            for (width, visits) in [32, 64, 128].into_iter().zip(visits) {
                assert!(
                    visits >= width,
                    "the reads must be observed on this thread: {probe} (named: {named}): \
                     {visits} at width {width}"
                );
            }
            for step in 0..2 {
                assert!(
                    visits[step + 1] * 10 <= visits[step] * 22,
                    "doubling the input at most doubles union reads: {probe} (named: \
                     {named}): {visits:?}"
                );
            }
        }
    }
}

/// A union the composite reduction proved it leaves as written is reused
/// only when every arm is a literal, a primitive or an enum member: that
/// canonicalization charges the connected demand nothing, so replaying the
/// proof prices a demand exactly as reducing the union again does. A union
/// holding any other arm is reduced again by every demand.
#[test]
fn a_reused_composite_proof_prices_a_demand_as_reducing_again() {
    use super::ProjectSemanticDispatch;
    use std::sync::Arc;
    use verter_type_engine::semantic_query::composite::CompositeList;
    use verter_type_engine::semantic_query::{
        AuthoredPropertyKey, LiteralValue, MacroOwnBodyStamp, MergeRoleStamp, PrimitiveKind,
        ProjectionReductionContext, SemanticNodeData, SurfaceMember,
    };
    type Dispatch<'a> = ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;

    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _builder = dispatch.enter_construction_scope();
    dispatch
        .set_connected_limits_for_tests(super::connected_demand::MAX_CONNECTED_PROJECTION_WORK, 24);
    let graph = dispatch.graph();
    let literals: Vec<_> = (0..16)
        .map(|index| {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                "s{index}"
            ))))
        })
        .collect();
    let union = |arms: Vec<_>| {
        graph.intern_node(SemanticNodeData::Union(
            CompositeList::authored_shell_for_tests(Arc::from(arms)),
        ))
    };
    let literal_union = union(literals.clone());
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let object = graph.intern_node(SemanticNodeData::Object(
        verter_type_engine::test_surface_view! {
            members: Arc::from(vec![SurfaceMember {
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                visibility: verter_type_expr::MemberVisibility::Public,
                key: AuthoredPropertyKey::string("x"),
                value: string,
                optional: false,
                readonly: false,
                method_kind: None,
                has_implementation_body: false,
                declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                merge_role: MergeRoleStamp::NEUTRAL,
                spans: Default::default(),
                declaration_origin: None,
            }].into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    let mixed_union = union(literals.iter().copied().chain([object]).collect());
    // Both reads of each union run in one demand, as the members of one
    // distribution do: each returns its own connected work, bytes and arm
    // reads.
    let (_guard, _) = dispatch.enter_connected_demand(false);
    let demand = |node| {
        let ledger = dispatch.connected_demand();
        let before = (
            ledger.work_used_for_tests(),
            ledger.bytes_used_for_tests(),
            Dispatch::composite_arm_reads_for_tests(),
        );
        let _ = dispatch.normalize_node_for_structural_fact_demand(
            node,
            ProjectionReductionContext::structural_transit(),
        );
        (
            ledger.work_used_for_tests() - before.0,
            ledger.bytes_used_for_tests() - before.1,
            Dispatch::composite_arm_reads_for_tests() - before.2,
        )
    };
    let cold = demand(literal_union);
    let replay = demand(literal_union);
    let first = demand(mixed_union);
    let again = demand(mixed_union);
    assert!(
        cold.2 >= literals.len(),
        "the cold reduction reads every arm: {cold:?}"
    );
    assert_eq!(
        replay.2, 0,
        "the proven union is not reduced again: {replay:?}"
    );
    assert!(
        first.2 > literals.len() && again.2 == first.2,
        "a union with an object arm is reduced by every demand: {first:?} {again:?}"
    );
    // A first read of either union also pays for settling the node, which
    // later reads are served; past it, reducing a union again and replaying
    // a proof charge the same.
    assert_eq!(
        (replay.0, replay.1),
        (again.0, again.1),
        "a replay charges what reducing again charges: {replay:?} {again:?}"
    );
}

/// A distributed member decided in place is charged and refused as its own
/// conditional query would be. Under every request budget from one that
/// refuses before the distribution finishes to one that answers whole, the
/// in-place decisions and the member queries give the same answer, the same
/// checker recovery and the same walker diagnostics.
#[test]
fn in_place_members_answer_and_refuse_as_member_queries_under_a_request_budget() {
    use std::sync::Arc;
    type Dispatch<'a> = super::ProjectSemanticDispatch<'a, crate::resolver_core::HostCapabilities>;
    let source = format!(
        "type Exclude<T, U> = T extends U ? never : T;\n{}type Most = Exclude<W, \"m3\" | \"m5\">;\n\
         type __Probe = Exclude<W, Most>;\nexport {{}};\n",
        wide_union(32)
    );
    // The answer's wire form, its checker recovery and the request's walker
    // diagnostics under `budget`, with every member its own query or not.
    let observe = |budget: usize, member_queries: bool| {
        let _forced = member_queries.then(Dispatch::force_member_queries_for_tests);
        let host = crate::VerterHost::new_standalone(crate::HostConfig {
            audit_enabled: true,
            projection_op_budget: budget,
            ..crate::HostConfig::default()
        });
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some("/scenario.ts".to_owned()),
                input_id: "/scenario.ts".to_owned(),
                source: Arc::from(source.as_str()),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert the scenario");
        let (outcome, audit) = host
            .resolve_named_symbol_with_audit("/scenario.ts", "__Probe", None)
            .into_parts();
        let answer = match outcome {
            Ok(Some(node)) => Some((
                host.project_node_to_type_expr_json_bytes(&node)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
                node.checker_recovery_code(),
            )),
            Ok(None) => None,
            Err(error) => panic!("budget {budget}: the request fails: {error:?}"),
        };
        let diagnostics = match &audit.kind_payload {
            verter_audit::RequestKindPayload::TypeResolution(payload) => {
                format!("{:?}", payload.walker_diagnostics)
            }
            other => panic!("not a type resolution audit: {other:?}"),
        };
        (answer, diagnostics)
    };
    let whole = observe(1_000_000, false);
    assert_eq!(whole, observe(1_000_000, true), "an unbounded request");
    let mut refused = 0;
    for budget in [1, 2, 4, 8, 16, 24, 32, 48, 64, 96, 128] {
        let in_place = observe(budget, false);
        assert_eq!(
            in_place,
            observe(budget, true),
            "budget {budget}: in-place members must answer as member queries do"
        );
        refused += usize::from(in_place != whole);
    }
    assert!(refused > 0, "some budget must refuse the distribution");
}

#[test]
fn a_partly_warm_literal_membership_demand_pays_preparation_once() {
    use super::ProjectSemanticDispatch;
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{
        LiteralValue, QueryResult, RelationOutcome, SemanticNodeData, SemanticQueryKey,
        SemanticQueryValue,
    };

    let observe =
        |warm_first: bool, allowance: usize, keys_to_read: std::ops::Range<usize>| {
            let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
            let dispatch = ProjectSemanticDispatch::new(&host);
            let _builder = dispatch.enter_construction_scope();
            let members: Vec<Arc<str>> = (0..32).map(|i| Arc::from(format!("s{i}"))).collect();
            let target = dispatch.intern_string_literal_union(&members);
            let keys =
                ["s0", "s1"].map(|value| {
                    let source = dispatch.graph().intern_node(SemanticNodeData::Literal(
                        LiteralValue::String(value.to_owned()),
                    ));
                    dispatch.relate_key_for(source, target).to_query_key()
                });
            let read = |key: &SemanticQueryKey| {
                let answer = dispatch.execute_via_cold_build_helper(key.clone());
                assert!(!answer.result_is_partial(), "{:?}", answer.value());
                assert!(
                    matches!(answer.value(),
                QueryResult::Value(SemanticQueryValue::Relation(payload))
                    if payload.outcome == RelationOutcome::Assignable),
                    "{:?}",
                    answer.value()
                );
            };
            if warm_first {
                read(&keys[0]);
            }
            dispatch.set_connected_limits_for_tests(allowance, 24);
            let usage = || {
                (
                    dispatch.connected_demand().work_used_for_tests(),
                    dispatch.connected_demand().bytes_used_for_tests(),
                )
            };
            let together = {
                let (_guard, _) = dispatch.enter_connected_demand(false);
                for key in &keys[keys_to_read] {
                    read(key);
                }
                usage()
            };
            // The cold sibling's receipt must still price preparation in full
            // when a later independent demand consumes it without the first.
            read(&keys[1]);
            (together, usage())
        };
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let cold = observe(false, full, 0..2);
    let partly_warm = observe(true, full, 0..2);
    assert_eq!(partly_warm, cold, "warmth changes neither work nor bytes");
    assert_eq!(
        cold.1,
        observe(false, full, 1..2).0,
        "the rebuilt receipt must price work and bytes like an independent cold demand"
    );
    assert_eq!(observe(false, cold.0 .0, 0..2), cold);
    assert_eq!(observe(true, cold.0 .0, 0..2), cold);
}

#[test]
fn literal_union_membership_preserves_values_order_and_general_members() {
    let source = r#"
        type W = "a" | "b" | "c" | 1 | 2 | true | false | 1n | 2n;
        enum E { A = "a", B = 1 }
        enum F { A = "a", B = 1 }
        type Pattern = `id:${number}`;
        type Obj = { value: number };
        type Mixed = "a" | E.A | E.B | Pattern | Obj;
    "#;
    let probes = [
        (
            "Exclude<W, 'a' | 'c' | 1 | true | 1n | 2n>",
            "\"b\" | 2 | false",
        ),
        (
            "Extract<W, 'a' | 'c' | 1 | true>",
            "\"a\" | \"c\" | 1 | true",
        ),
        ("Exclude<'a' | 'b' | 'c', 'c'>", "\"b\" | \"a\""),
        ("Exclude<'id:3' | 'other', Mixed>", "\"other\""),
        (
            "[Extract<1n | 2n, 1n>] extends [1n] ? [1n] extends [Extract<1n | 2n, 1n>] ? 1 : 2 : 2",
            "1",
        ),
        (
            "[Exclude<1n | 2n, 1n>] extends [2n] ? [2n] extends [Exclude<1n | 2n, 1n>] ? 1 : 2 : 2",
            "1",
        ),
        ("[Obj] extends [Mixed] ? 1 : 2", "1"),
        ("[E.A] extends [Mixed] ? 1 : 2", "1"),
        ("[F.A] extends [E.A | E.B] ? 1 : 2", "2"),
        ("['a'] extends [E.A | E.B] ? 1 : 2", "2"),
        ("[1] extends [E.A | E.B] ? 1 : 2", "1"),
        ("[boolean] extends [true | false] ? 1 : 2", "1"),
        ("['1'] extends [1 | 2] ? 1 : 2", "2"),
        ("[1n] extends [1 | 2] ? 1 : 2", "2"),
    ];
    for options in [
        r#"{"strictNullChecks":true,"noImplicitAny":true}"#,
        r#"{"strictNullChecks":true,"noImplicitAny":false}"#,
        r#"{"strictNullChecks":false,"noImplicitAny":true}"#,
        r#"{"strictNullChecks":false,"noImplicitAny":false}"#,
    ] {
        let failures = super::checker_probe_lane_tests::mismatches_in(
            super::checker_probe_lane_tests::ProbeProject {
                compiler_options: Some(options),
                ..Default::default()
            },
            source,
            &probes,
        );
        assert!(failures.is_empty(), "{options}: {}", failures.join("\n"));
    }
}

/// A literal union source related member by member to a literal union
/// target, and distributions through a declared `Exclude` / `Extract`
/// over literal unions, a derived one among them, answer as the checker
/// does. Measured on TypeScript 7.0.2 under each of `strictNullChecks` and
/// `noImplicitAny` on and off (the four settings agree on every probe).
#[test]
fn literal_union_sources_and_declared_distributions_answer_as_the_checker() {
    let source = r#"
        type W = "a" | "b" | "c" | 1 | 2 | true | false | 1n | 2n;
        enum E { A = "a", B = 1 }
        type MyExclude<T, U> = T extends U ? never : T;
        type MyExtract<T, U> = T extends U ? T : never;
        type Most = MyExclude<W, "c" | 2>;
    "#;
    let probes = [
        ("['a' | 1 | true | 2n] extends [W] ? 1 : 2", "1"),
        ("['a' | 'zz'] extends [W] ? 1 : 2", "2"),
        ("['1' | 2] extends [1 | 2] ? 1 : 2", "2"),
        ("[1 | 2] extends [1 | 2 | 3] ? 1 : 2", "1"),
        ("[1n | 3n] extends [W] ? 1 : 2", "2"),
        ("['a' | 'b'] extends [E.A | E.B] ? 1 : 2", "2"),
        ("MyExclude<'a' | 'b' | 1, 'a' | 1>", "\"b\""),
        ("MyExtract<'a' | 'b' | 1, 'a' | 1 | 3>", "\"a\" | 1"),
        ("MyExclude<W, Most>", "\"c\" | 2"),
        ("[Most] extends [W] ? 1 : 2", "1"),
        ("[W] extends [Most] ? 1 : 2", "2"),
        ("[true | 'a'] extends [boolean | 'a'] ? 1 : 2", "1"),
        ("[1 | 'b'] extends [MyExclude<W, 'a'>] ? 1 : 2", "1"),
        ("MyExclude<1 | 2 | 'b', MyExclude<W, 'a'>>", "never"),
    ];
    for options in [
        r#"{"strictNullChecks":true,"noImplicitAny":true}"#,
        r#"{"strictNullChecks":true,"noImplicitAny":false}"#,
        r#"{"strictNullChecks":false,"noImplicitAny":true}"#,
        r#"{"strictNullChecks":false,"noImplicitAny":false}"#,
    ] {
        let failures = super::checker_probe_lane_tests::mismatches_in(
            super::checker_probe_lane_tests::ProbeProject {
                compiler_options: Some(options),
                ..Default::default()
            },
            source,
            &probes,
        );
        assert!(failures.is_empty(), "{options}: {}", failures.join("\n"));
    }
}

#[test]
fn literal_union_membership_keeps_canonical_output_order() {
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};
    with_probe("", "Exclude<'a' | 'b' | 'c', 'c'>", |dispatch, node| {
        let data = dispatch.graph().node_data(node).expect("complete union");
        let SemanticNodeData::Union(members) = data.as_ref() else {
            panic!("expected a union: {data:?}");
        };
        let values: Vec<_> = members
            .iter()
            .map(
                |member| match dispatch.graph().node_data(*member).as_deref() {
                    Some(SemanticNodeData::Literal(LiteralValue::String(value))) => value.clone(),
                    other => panic!("expected a string literal: {other:?}"),
                },
            )
            .collect();
        assert_eq!(values, ["b", "a"]);
    });
}

#[test]
fn literal_union_membership_revalidates_imported_values_after_edits() {
    use super::checker_probe_lane_tests::{
        default_probe_host, upsert_probe_files, with_probe_on_host,
    };
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};
    let host = default_probe_host();
    let source = "import type { Most } from './targets'; type W = 'a' | 'b' | 'c';";
    for (target, expected) in [
        ("export type Most = 'a' | 'c';", "b"),
        ("export type Most = 'a' | 'b';", "c"),
    ] {
        upsert_probe_files(&host, &[("targets.ts", target)]);
        for _ in 0..2 {
            with_probe_on_host(
                &host,
                Default::default(),
                source,
                "Exclude<W, Most>",
                |dispatch, node| {
                    assert!(matches!(dispatch.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Literal(LiteralValue::String(value))) if value == expected));
                },
            );
        }
    }
}

fn check_large_literal_membership(width: usize, probe: &str, expected: &str) {
    let members: Vec<_> = (0..width).map(|index| format!("'s{index}'")).collect();
    let most: Vec<_> = members
        .iter()
        .enumerate()
        .filter(|(index, _)| ![3, 5].contains(index))
        .map(|(_, member)| member.as_str())
        .collect();
    let source = format!(
        "type W = {}; type Most = {};",
        members.join(" | "),
        most.join(" | ")
    );
    let failures = mismatches(&source, &[(probe, expected)]);
    assert!(
        failures.is_empty(),
        "{width} members: {}",
        failures.join("\n")
    );
}

#[test]
fn exclude_near_equal_1000_literals_keeps_the_two_missing_values() {
    check_large_literal_membership(1000, "Exclude<W, Most>", "\"s3\" | \"s5\"");
}

#[test]
fn exclude_near_equal_3000_literals_keeps_the_two_missing_values() {
    check_large_literal_membership(3000, "Exclude<W, Most>", "\"s3\" | \"s5\"");
}

#[test]
fn subset_of_2000_literals_relates_to_the_whole_union() {
    check_large_literal_membership(2000, "[Most] extends [W] ? 'sub' : 'other'", "\"sub\"");
}

#[test]
fn subset_of_5000_literals_relates_to_the_whole_union() {
    check_large_literal_membership(5000, "[Most] extends [W] ? 'sub' : 'other'", "\"sub\"");
}

/// Measured on TypeScript 7.0.2 (all four settings): over an 800-member
/// `W`, `[Exclude<W, "m0">] extends [W] ? 1 : 2` is `1` and `[W] extends
/// [Exclude<W, "m0">] ? 1 : 2` is `2`.
#[test]
fn excluding_one_member_of_an_800_member_union_relates_back_to_it() {
    let failures = mismatches(
        &wide_union(800),
        &[
            (EXCLUDED_TO_WHOLE, "1"),
            ("[W] extends [Exclude<W, \"m0\">] ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Relating `Exclude<W, "m0">` back to `W` makes the same number of
/// relation checks per added member at 100, 200 and 300 members.
///
/// Oracle: as for
/// [`excluding_one_member_of_an_800_member_union_relates_back_to_it`], at
/// every width.
#[test]
fn relating_a_wide_literal_union_to_its_superset_costs_the_same_checks_per_member() {
    let checks = |width: usize| {
        with_probe(&wide_union(width), EXCLUDED_TO_WHOLE, |dispatch, _| {
            dispatch.graph().stats_snapshot().relation_check_count
        })
    };
    let (at_100, at_200, at_300) = (checks(100), checks(200), checks(300));
    assert_eq!(
        at_200 - at_100,
        at_300 - at_200,
        "relation checks at 100 / 200 / 300 members: {at_100} / {at_200} / {at_300}"
    );
}

/// `type K = "k0" | … `, `width` members, and `last(x: K)` returning `x`
/// past a `throw` on each of its first `width - 1` members.
fn equality_guard_chain(width: usize) -> String {
    let members: Vec<String> = (0..width).map(|index| format!("\"k{index}\"")).collect();
    let mut guards = String::new();
    for index in 0..width - 1 {
        guards.push_str(&format!("  if (x === \"k{index}\") throw 0;\n"));
    }
    format!(
        "type K = {};\nexport function last(x: K) {{\n{guards}  return x;\n}}\n",
        members.join(" | ")
    )
}

/// A chain of `x === "k<i>"` guards over a literal union filters every
/// remaining arm against each guard's literal, as the checker's
/// `filterType` does; each such pair is two literal types, decided by
/// value, so no relation of the chain reaches the structural reducer.
///
/// Measured on TypeScript 7.0.2 (`--declaration --emitDeclarationOnly`,
/// all four settings): `last` is `"k799"` over an 800-member union and
/// `"k99"` over a 100-member one.
#[test]
fn an_equality_guard_chain_over_a_literal_union_reduces_no_relation() {
    let before = super::ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests();
    let failures = mismatches(
        &equality_guard_chain(100),
        &[("ReturnType<typeof last>", "\"k99\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        super::ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests() - before,
        0,
        "every relation a literal guard chain asks is a pair of literals"
    );
    let failures = mismatches(
        &equality_guard_chain(800),
        &[("ReturnType<typeof last>", "\"k799\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
