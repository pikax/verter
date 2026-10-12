//! A conditional's check and extends types are the types the checker
//! constructs before it relates them: a union or intersection whose members
//! name a lattice extreme is that extreme (`1 & T` over `T = any` is `any`),
//! whatever carrier the composite was built as, and an operand that is the
//! checker's error type is the conditional's answer.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on [`FIXTURE`], each probe read off a
//! TS2322 against `never`. The four `strictNullChecks` × `noImplicitAny`
//! settings agree on every probe. The checker prints its error type (here
//! the TS2589 recovery of `Same<0>`) as `any`.

use super::checker_probe_lane_tests::mismatches_in_one_host;

const FIXTURE: &str = r#"
type IsAny<T> = 0 extends 1 & T ? "any" : "not-any";
type A = any;
type U = unknown;
type Rep<N extends number, Acc extends unknown[] = []> = Acc["length"] extends N ? Acc : Rep<N, [...Acc, 0]>;
type Same<T> = T extends 0 ? Same<T> : 1;
type E = Same<0>;
type Box<T> = { v: T };
type InU<T> = 0 extends 1 | T ? "in" : "out";
type Nev<T> = 0 extends 1 & T & never ? "y" : "n";
"#;

/// An intersection substituted into an instantiated body is the intersection
/// the checker constructs: `1 & T` over `any` is `any`, over the error type
/// the error type, and `never` beside `any` wins.
///
/// Measured: `IsAny<A>`, `IsAny<any>` and `IsAny<Box<any>["v"]>` are
/// `"any"`; `IsAny<U>`, `IsAny<never>` and `IsAny<string>` are
/// `"not-any"`; `IsAny<E>` and `IsAny<Rep<1000>>` are `any` (the error
/// type); `InU<A>` is `"in"`, `InU<E>` is `any`; `Nev<A>` is `"n"`.
/// `Rep<1000>` is TS2589 only at the checker's tail limit: Verter runs past
/// it to the 1,000-element tuple, so its `IsAny<Rep<1000>>` is `"not-any"`.
#[test]
fn a_composite_operand_is_the_extreme_its_members_name() {
    let failures = mismatches_in_one_host(
        FIXTURE,
        &[
            ("IsAny<A>", r#""any""#),
            ("IsAny<any>", r#""any""#),
            ("IsAny<Box<any>[\"v\"]>", r#""any""#),
            ("IsAny<U>", r#""not-any""#),
            ("IsAny<never>", r#""not-any""#),
            ("IsAny<string>", r#""not-any""#),
            ("IsAny<E>", "any"),
            ("IsAny<Rep<1000>>", r#""not-any""#),
            ("InU<A>", r#""in""#),
            ("InU<E>", "any"),
            ("Nev<A>", r#""n""#),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_approximate_extreme_cannot_select_a_conditional_branch() {
    use super::checker_probe_lane_tests::{
        default_probe_host, flow_return_audited_on_host, with_probe_outcome_on_host, ProbeProject,
    };
    use verter_type_engine::project_semantic_dispatch::flow_return::flow_admission_fault_injection;
    use verter_type_engine::semantic_query::{
        BranchSelection, FactStatus, LiteralValue, OriginEdgeKind, OriginMeta, SemanticNodeData,
    };

    const SOURCE: &str = "export function f(x: any) { return x; }";
    const PROBE: &str = "0 extends 1 & ReturnType<typeof f> ? 'selected' : 'other'";
    for refused in [true, false] {
        let host = default_probe_host();
        let _refusal = refused.then(|| {
            flow_admission_fault_injection::Guard::arm(
                &host.flow_fault_injection.zero_obligation_budget,
            )
        });
        let returned = flow_return_audited_on_host(&host, SOURCE, "f")
            .into_result()
            .expect("proof refusal retains the evaluated any return");
        assert_eq!(
            matches!(returned.fact().status(), FactStatus::Approximate(_)),
            refused,
            "the source control must distinguish proof refusal from an exact any"
        );
        with_probe_outcome_on_host(
            &host,
            ProbeProject::default(),
            SOURCE,
            PROBE,
            |dispatch, outcome| {
                let decisions = dispatch
                    .graph()
                    .export_all_origin_edges()
                    .into_iter()
                    .filter(|(_, kind, edge)| {
                        *kind == OriginEdgeKind::ConditionalSelect
                            && matches!(
                                edge.meta,
                                OriginMeta::Branch(BranchSelection::True | BranchSelection::False)
                            )
                    })
                    .collect::<Vec<_>>();
                if refused {
                    assert!(
                        decisions.is_empty(),
                        "an approximate extreme must not establish a conditional branch: {decisions:?}"
                    );
                    assert!(outcome.into_complete_node().is_none());
                } else {
                    assert!(
                        !decisions.is_empty(),
                        "the exact control selects its branch"
                    );
                    let node = outcome
                        .into_complete_node()
                        .expect("the exact control completes");
                    assert!(matches!(
                        dispatch.graph().node_data(node).as_deref(),
                        Some(SemanticNodeData::Literal(LiteralValue::String(value)))
                            if value.as_str() == "selected"
                    ));
                }
            },
        );
    }
}
