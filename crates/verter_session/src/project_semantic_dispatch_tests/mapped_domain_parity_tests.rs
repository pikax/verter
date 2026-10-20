//! A mapped type evaluates the way TypeScript's `instantiateMappedType` and
//! `resolveMappedTypeMembers` do over key domains beyond a plain object's
//! keys: an enum, a function type, a tuple's keys under an `as` clause, an
//! `as` filter that reads the member value, `any`, and a mapped type read
//! through another mapped type.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this exact
//! fixture with `tsc --ignoreConfig --noEmit --strict`: `declare const v:
//! <probe>; export const s: null = v;` read off the TS2322 message, and a
//! print naming an alias (`ByEnum`, `Partial<Fn>`) checked against the
//! expected type with `(<T>() => T extends A ? 1 : 2) extends (<T>() => T
//! extends B ? 1 : 2)`.

use super::checker_probe_lane_tests::{evaluated_mismatches, mismatches, with_probe};
use verter_type_engine::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeData,
};

const MAPPED: &str = "\
type W0 = 5;
type W1 = { v: W0 }; type W2 = { v: W1 }; type W3 = { v: W2 }; type W4 = { v: W3 };
type W5 = { v: W4 }; type W6 = { v: W5 }; type W7 = { v: W6 }; type W8 = { v: W7 };
type W9 = { v: W8 }; type W10 = { v: W9 };
type Big = { p0: W10; p1: W10; p2: W10; p3: W10; p4: W10; p5: W10; p6: W10; p7: W10 };
type RO<T> = { readonly [K in keyof T]: T[K] };
type RB = RO<Big>;
enum E { A, B, C }
type ByEnum = { [K in E]: K };
type Fn = () => void;
type Tup = [1, 'a'];
type TupleKeys = { [K in keyof Tup as K extends `${number}` ? K : never]: Tup[K] };
type NoFns<T> = { [K in keyof T as T[K] extends Function ? never : K]: T[K] };
type Mp<T> = { [K in keyof T]: T[K] };
type NoDates<T> = { [K in keyof T as T[K] extends Date ? never : K]: T[K] };
enum S { A = 'a', B = 'b' }
";

/// `RO<RO<Big>>['p5']` reads `Big['p5']` through two mappings — the outer
/// one's source an application of the inner, whose keys its declaration
/// does not write — and every further hop reads through `W10`'s own
/// members: ten `v` hops reach `5`. The same holds through an alias of the
/// inner application.
#[test]
fn a_mapped_type_indexed_after_another_mapped_type_evaluates() {
    let failures = mismatches(
        MAPPED,
        &[
            (
                "RO<RO<Big>>['p5']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']",
                "5",
            ),
            (
                "RO<RB>['p5']['v']['v']['v']['v']['v']['v']['v']['v']['v']['v']",
                "5",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A mapped type over an enum keys each member by its value and binds the
/// member itself: `{ [K in E]: K }` is `{ 0: E.A; 1: E.B; 2: E.C; }`, and an
/// `as` remap reads the member too.
#[test]
fn a_mapped_type_over_an_enum_domain_enumerates_its_members() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("ByEnum", "{ 0: E.A; 1: E.B; 2: E.C; }"),
            (
                "{ [K in E as `k${K}`]: K }",
                "{ k0: E.A; k1: E.B; k2: E.C; }",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A function type has no properties: `keyof (() => void)` is `never`, and
/// a mapping over it — homomorphic through a type parameter or written over
/// it directly — is `{}`.
#[test]
fn a_homomorphic_mapping_over_a_function_type_is_empty() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("Partial<Fn>", "{}"),
            ("Mp<() => void>", "{}"),
            ("Mp<Fn>", "{}"),
            ("{ [K in keyof Fn]: Fn[K] }", "{}"),
            ("keyof (() => void)", "never"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A tuple's keys remapped through `as` are a plain object of its numeric
/// keys: `length`, the array methods and the numeric index are filtered out.
#[test]
fn a_tuple_key_domain_remapped_with_as_keeps_its_element_keys() {
    let failures = evaluated_mismatches(MAPPED, &[("TupleKeys", "{ \"0\": 1; \"1\": \"a\"; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `as` clause that reads the member value filters function-valued
/// members out: a literal is no `Function`.
#[test]
fn an_as_filter_on_function_valued_members_drops_them() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("NoFns<{ a: 1; g: () => void }>", "{ a: 1; }"),
            ("1 extends Function ? 'y' : 'n'", "\"n\""),
            ("NoFns<{ a: { x: 1 }; g: () => void }>", "{ a: { x: 1; }; }"),
            ("{ x: 1 } extends Function ? 'y' : 'n'", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `as` filter whose relation the lane cannot decide (a member value
/// against a global nominal with no body, `Date extends Function`, or any
/// operand against `Date`) keeps the deferred mapped carrier, and an index
/// into it stays a deferred indexed access: never a failure node.
#[test]
fn an_undecided_as_filter_keeps_the_deferred_mapped_carrier() {
    let failures: Vec<String> = [
        ("NoFns<{ a: Date; g: () => void }>", false),
        ("NoFns<{ a: Map<1, 2>; g: () => void }>", false),
        ("NoDates<{ a: 1; d: Date }>", false),
        ("NoDates<{ a: 1; p: Promise<1> }>", false),
        (
            "{ [K in keyof { a: 1; d: Date } as { a: 1; d: Date }[K] extends Date ? never : K]: 1 }",
            false,
        ),
        ("NoDates<{ a: 1; d: Date }>['a']", true),
    ]
    .into_iter()
    .filter_map(|(probe, indexed)| {
        with_probe(MAPPED, probe, |dispatch, node| {
            let value = dispatch
                .normalize_node_for_structural_fact_demand(
                    node,
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                )
                .into_usable_node()
                .unwrap_or_else(|| panic!("the probe `{probe}` evaluated to a partial demand"));
            let data = dispatch.graph().node_data(value);
            let deferred = match data.as_deref() {
                Some(SemanticNodeData::IndexedAccess { .. }) => indexed,
                Some(SemanticNodeData::Mapped { .. }) => !indexed,
                _ => false,
            };
            (!deferred).then(|| format!("`{probe}` evaluated to {data:?}"))
        })
    })
    .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An enum-keyed record read by an enum member reads the property the
/// member's value names, for a numeric and a string enum alike.
#[test]
fn an_enum_keyed_mapping_indexed_by_an_enum_member_reads_its_property() {
    let failures = mismatches(
        MAPPED,
        &[
            ("ByEnum[E.A]", "E.A"),
            ("Record<E, string>[E.B]", "string"),
            ("Record<S, 1>[S.A]", "1"),
            ("Mp<Record<E, 2>>[E.C]", "2"),
            ("ByEnum[0]", "E.A"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A homomorphic mapping over `any` maps the one key type `any` has,
/// `string`: an index signature of the template's value, not `any`.
#[test]
fn a_homomorphic_mapping_over_any_is_an_index_signature() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("Mp<any>", "{ [x: string]: any; }"),
            ("Partial<any>", "{ [x: string]: any; }"),
            ("{ [K in keyof any]: 1 }", "{ [x: string]: 1; }"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A mapping over a closed object keeps its answer.
#[test]
fn a_mapping_over_a_closed_object_keeps_its_answer() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("Mp<{ a: 1; b: 'x' }>", "{ a: 1; b: \"x\"; }"),
            ("RO<{ a: 1 }>", "{ readonly a: 1; }"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
