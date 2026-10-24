//! A nested function value prepared again on an equal basis within one
//! root evaluation reuses the signature its first evaluation composed: an
//! options-API call evaluates its argument's methods once per distinct
//! basis (the context-free argument and its const-context view share one;
//! the retyping under the contextual `this` is another), and a basis that
//! differs where the body reads it — a receiver member, the contextual
//! `this`, the method body after an edit — evaluates the body again.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture under `--strict` with `export const a: { __nope: 1 } = null as
//! any as <probe>;` read off the TS2741 message.

use super::checker_probe_lane_tests::{
    default_probe_host, mismatches_in_one_host_observing, mismatches_on_host, ProbeProject,
};

const MARKER: &str = "export {}; declare global { interface ThisType<T> {} }\n";

const DEFINE_COMPONENT: &str = "declare function defineComponent<D, M>(o: { data(): D; methods: M & ThisType<D & M> }): { [K in keyof M]: M[K] extends () => infer R ? R : never };\n";

/// An options-API component of `count` methods, each returning the data
/// member of its index, and its checker print.
fn component(count: usize) -> (String, String) {
    let data: Vec<String> = (0..count).map(|i| format!("d{i}: {i} as const")).collect();
    let methods: Vec<String> = (0..count)
        .map(|i| format!("m{i}() {{ return this.d{i}; }}"))
        .collect();
    let print: Vec<String> = (0..count).map(|i| format!("m{i}: {i};")).collect();
    (
        format!(
            "{MARKER}{DEFINE_COMPONENT}const comp = defineComponent({{ data() {{ return {{ {} }}; }}, methods: {{ {} }} }});\n",
            data.join(", "),
            methods.join(", ")
        ),
        format!("{{ {} }}", print.join(" ")),
    )
}

/// The nested bodies one cold demand of `typeof comp` evaluates.
fn component_body_evaluations(count: usize) -> usize {
    let (source, print) = component(count);
    let before = super::flow_return::nested_body_evaluations_for_tests();
    let failures =
        mismatches_in_one_host_observing(&source, &[("typeof comp", print.as_str())], |_| {});
    assert!(failures.is_empty(), "{source}\n{}", failures.join("\n"));
    super::flow_return::nested_body_evaluations_for_tests() - before
}

/// Each method body evaluates once per distinct basis: once context-free
/// (the argument's value and its const-context view read it on the same
/// basis) and once under the contextual `this` of `D & M`. Every method
/// the component adds adds those two evaluations, not three.
#[test]
fn an_options_api_method_body_evaluates_once_per_distinct_basis() {
    let [eight, sixteen, thirty_two] = [8, 16, 32].map(component_body_evaluations);
    assert_eq!(
        sixteen - eight,
        2 * 8,
        "two evaluations per added method: {eight}, {sixteen}, {thirty_two}"
    );
    assert_eq!(
        thirty_two - sixteen,
        2 * 16,
        "two evaluations per added method: {eight}, {sixteen}, {thirty_two}"
    );
    assert!(
        eight <= 2 * 8 + 4,
        "eight methods and the data function evaluate {eight} bodies"
    );
}

/// A method of a literal passed for a constrained type parameter reads
/// `this` through the literal's own receiver, the same basis in every
/// pass, but the receiver's `x` differs: `number` context-free, the
/// literal the constraint keeps under the contextual type. The body read
/// against the contextual pass's receiver is evaluated again and returns
/// the literal.
#[test]
fn a_receiver_member_read_differently_evaluates_the_body_again() {
    let source = format!(
        "{MARKER}declare function numeric<T extends {{ x: 1 | 2 }}>(o: T): T;\n\
         declare function textual<T extends {{ x: \"a\" | \"b\" }}>(o: T): T;\n\
         const n = numeric({{ x: 1, m() {{ return this.x; }} }});\n\
         const t = textual({{ x: \"a\", m() {{ return [this.x]; }} }});\n"
    );
    let failures = mismatches_in_one_host_observing(
        &source,
        &[
            ("typeof n", "{ x: 1; m(): 1; }"),
            ("typeof t", "{ x: \"a\"; m(): \"a\"[]; }"),
        ],
        |_| {},
    );
    assert!(failures.is_empty(), "{source}\n{}", failures.join("\n"));
}

/// Contextual `this` through `ThisType<D & M>` and the return conditional
/// over each method: a method reading another method reads its inferred
/// return, whichever literal the data member holds.
#[test]
fn contextual_this_methods_read_data_and_sibling_methods() {
    let source = format!(
        "{MARKER}{DEFINE_COMPONENT}const comp = defineComponent({{ data() {{ return {{ d0: 0 as const, d1: \"a\" as const }}; }}, methods: {{ m0() {{ return this.d0; }}, m1() {{ return this.d1; }}, m2() {{ return this.m0(); }} }} }});\n"
    );
    let failures = mismatches_in_one_host_observing(
        &source,
        &[("typeof comp", "{ m0: 0; m1: \"a\"; m2: 0; }")],
        |_| {},
    );
    assert!(failures.is_empty(), "{source}\n{}", failures.join("\n"));
}

/// An edit to a method body, and one to the contextual `this` type, change
/// the answer the same host gives after it.
#[test]
fn an_edit_to_a_method_body_or_the_contextual_this_changes_the_answer() {
    let host = default_probe_host();
    let project = ProbeProject::default();
    let methods = "methods: { m0() { return this.d0; }, m1() { return this.d1; }, m2() { return this.m0(); } }";
    let data = "data() { return { d0: 0 as const, d1: \"a\" as const }; }";
    let original = format!(
        "{MARKER}{DEFINE_COMPONENT}const comp = defineComponent({{ {data}, {methods} }});\n"
    );
    let body_edited = original.replace("m0() { return this.d0; }", "m0() { return this.d1; }");
    let this_edited = original.replace(
        "methods: M & ThisType<D & M>",
        "methods: M & ThisType<{ d0: 5; d1: 6 } & M>",
    );
    let mut failures = Vec::new();
    for (source, print) in [
        (&original, "{ m0: 0; m1: \"a\"; m2: 0; }"),
        (&body_edited, "{ m0: \"a\"; m1: \"a\"; m2: \"a\"; }"),
        (&this_edited, "{ m0: 5; m1: 6; m2: 5; }"),
        (&original, "{ m0: 0; m1: \"a\"; m2: 0; }"),
    ] {
        failures.extend(mismatches_on_host(
            &host,
            project,
            source,
            &[("typeof comp", print)],
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Body evaluations and the outcome of `probe` over `source` on a fresh
/// host `arm` configured.
fn evaluations_on(
    source: &str,
    probe: &str,
    arm: impl FnOnce(&crate::VerterHost),
) -> (usize, std::sync::Arc<crate::VerterHost>, Option<String>) {
    let host = default_probe_host();
    arm(&host);
    let before = super::flow_return::nested_body_evaluations_for_tests();
    let rendered = super::checker_probe_lane_tests::with_probe_outcome_on_host(
        &host,
        ProbeProject::default(),
        source,
        probe,
        |dispatch, outcome| {
            outcome.into_usable_node().map(|node| {
                crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                    dispatch, node, 0,
                )
            })
        },
    );
    (
        super::flow_return::nested_body_evaluations_for_tests() - before,
        host,
        rendered,
    )
}

/// A loop's second pass reuses the signature of the callback its first pass
/// evaluated, and its call's evidence keeps the relation outcomes the
/// callback's calls consumed: every call here consumed an undecided one, so
/// no recorded call claims its relations decided, the reused pass's
/// included.
#[test]
fn a_reused_body_keeps_the_undecided_relations_its_calls_consumed() {
    use std::sync::atomic::Ordering;
    let source = "declare function inner(x: number): number;\n\
                  declare function g<T>(cb: () => T): T;\n\
                  export function lp(n: number) { let r = 0; for (let i = 0; i < n; i++) { r = g(() => inner(1)); } return r; }\n";
    let reuses = super::flow_return::nested_signature_reuses_for_tests();
    let (_, host, rendered) = evaluations_on(source, "ReturnType<typeof lp>", |host| {
        let knobs = &host.flow_fault_injection;
        knobs.observe_call_evidence.store(true, Ordering::Relaxed);
        knobs
            .undecided_nested_call_relations
            .store(true, Ordering::Relaxed);
    });
    assert_eq!(rendered.as_deref(), Some("number"));
    assert!(
        super::flow_return::nested_signature_reuses_for_tests() > reuses,
        "the loop's second pass reuses the callback"
    );
    let evidence = host
        .flow_fault_injection
        .call_evidence
        .lock()
        .unwrap()
        .clone();
    assert!(!evidence.is_empty());
    assert!(
        evidence.iter().all(|decided| !decided),
        "no call claims decided relations: {evidence:?}"
    );
}

/// A capture whose reading degrades the frame (a `let` read before its
/// declarator ran) keeps its function out of the memo, so every pass of
/// the loop reads the capture and records the degradation where it reads
/// it; the same loop reading a variable declared before it reuses the
/// callback from its second pass.
#[test]
fn a_degrading_capture_is_read_by_every_preparation() {
    let reused = |source: &str| {
        let before = super::flow_return::nested_signature_reuses_for_tests();
        let (_, _, rendered) = evaluations_on(source, "ReturnType<typeof dg>", |_| {});
        assert_eq!(rendered.as_deref(), Some("number"), "{source}");
        super::flow_return::nested_signature_reuses_for_tests() - before
    };
    assert_eq!(
        reused(
            "declare function g<T>(cb: () => T): T;\n\
             export function dg(n: number) { let r = 0; for (let i = 0; i < n; i++) { r = g(() => z); } let z = 1; return r; }\n"
        ),
        0
    );
    assert!(
        reused(
            "declare function g<T>(cb: () => T): T;\n\
             export function dg(n: number) { let z = 1; let r = 0; for (let i = 0; i < n; i++) { r = g(() => z); } return r; }\n"
        ) > 0
    );
}

/// A nested function value whose slice content cannot be read returns
/// before its captures are read: the destructured parameter element it
/// captures is not seeded on the frame creating it. Read normally, the
/// capture seeds it.
#[test]
fn a_preparation_that_returns_early_leaves_its_frame_unchanged() {
    use std::sync::atomic::Ordering;
    let source = "export function sd({ a }: { a: number }) { return () => a; }\n";
    let seeds = |withheld: bool| {
        let (_, host, _) = evaluations_on(source, "ReturnType<typeof sd>", |host| {
            host.flow_fault_injection
                .fail_nested_slice_content
                .store(withheld, Ordering::Relaxed);
        });
        host.flow_fault_injection
            .nested_capture_seeds
            .load(Ordering::Relaxed)
    };
    assert_eq!(seeds(true), 0);
    assert!(seeds(false) > 0);
}
