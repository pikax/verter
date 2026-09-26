//! A union or intersection reduces over its arms' resolved types, as the
//! checker's `getUnionType` / `getIntersectionType` do: an arm written as
//! a name (an alias, an enum, an application) takes part in literal
//! subsumption, the `any` / `never` lattice and disjoint-literal collapse
//! exactly as the type it names.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with
//! `--declaration --emitDeclarationOnly` over `declare const x: <probe>;
//! export const y = x;` under all four `strictNullChecks` × `noImplicitAny`
//! settings; every answer is the same under all four.

use super::checker_probe_lane_tests::mismatches;

/// An intersection with an arm that resolves to `any` is `any`, so a
/// conditional over it takes both branches' union.
///
/// Measured on TypeScript 7.0.2: over `declare function anyf(): any`, `1 &
/// ReturnType<typeof anyf>` is `any` and `0 extends 1 & ReturnType<typeof
/// anyf> ? 1 : 0` is `1`.
#[test]
fn an_intersection_with_an_arm_resolving_to_any_is_any() {
    let failures = mismatches(
        "export declare function anyf(): any;\n",
        &[
            ("1 & ReturnType<typeof anyf>", "any"),
            ("0 extends 1 & ReturnType<typeof anyf> ? 1 : 0", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NAMED: &str = "\
export type A = 1 | 2;
export type S = \"a\" | \"b\" | 3;
export type N = number;
export enum E { A, B }
export interface I { a: 1 }
";

/// A union reduces over its named arms' types and keeps the names the
/// checker keeps as its origin.
///
/// Measured on TypeScript 7.0.2: `A | number` is `number`, `A | string`
/// `string | A`, `A | 1` `A`, `A | S` `A | S`, `A | 3 | S` `A | S`, `A | 1 | 3` `3 | A`, `N | 1` `number`, `E | number`
/// `number`, `E | string` `string | E`, `E | E.A` `E`, `E | 5` `5 | E`.
#[test]
fn a_union_reduces_over_its_named_arms() {
    let failures = mismatches(
        NAMED,
        &[
            ("A | number", "number"),
            ("A | string", "string | A"),
            ("A | 1", "A"),
            ("A | S", "A | S"),
            ("A | 3 | S", "A | S"),
            ("A | 1 | 3", "3 | A"),
            ("N | 1", "number"),
            ("E | number", "number"),
            ("E | string", "string | E"),
            ("E | E.A", "E"),
            ("E | 5", "5 | E"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An intersection reduces over its named arms' types: literals over their
/// primitives, disjoint unit types to `never`, and distribution over a
/// named union; an object arm keeps the intersection as written.
///
/// Measured on TypeScript 7.0.2: `A & 1` is `1`, `A & (1 | 3)` `1`, `S &
/// string` `"a" | "b"`, `E & number` `E`, `E & E.A` `E.A`, `E & 0`
/// `never`, `A & 3` `never`, and `I & { a: 1 }` `I & { a: 1; }`.
#[test]
fn an_intersection_reduces_over_its_named_arms() {
    let failures = mismatches(
        NAMED,
        &[
            ("A & 1", "1"),
            ("A & (1 | 3)", "1"),
            ("S & string", "\"a\" | \"b\""),
            ("E & number", "E"),
            ("E & E.A", "E.A"),
            ("E & 0", "never"),
            ("A & 3", "never"),
            ("I & { a: 1 }", "I & { a: 1; }"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union whose named arm is an alias naming itself through an array
/// reduces over the alias's members without re-entering it.
///
/// Measured on TypeScript 7.0.2 over `type R = R[] | 1`: `R | number` is
/// `number | R[]`, `R & 1` `R & 1`, and `1 extends R ? "y" : "n"` `"y"`.
#[test]
fn a_union_over_a_self_referencing_alias_reduces_once() {
    let failures = mismatches(
        "export type R = R[] | 1;\n",
        &[
            ("R | number", "number | R[]"),
            ("R & 1", "R & 1"),
            ("1 extends R ? \"y\" : \"n\"", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Mutually recursive alias unions are the checker's circularity error
/// (TS2456), typed `any`: `MA | number`, `MB | 3`, `MA | 1` and `MA & 1`
/// are `any` on TypeScript 7.0.2. The reduction meets a name on its own
/// active path and leaves each composite as written, so the read
/// terminates, on the default test stack, and never publishes a reduced
/// member set for the cycle.
#[test]
fn mutually_recursive_alias_unions_stay_as_written() {
    let source = "\
export type MA = MB | 1;
export type MB = MA | 2;
";
    for (probe, written) in [
        ("MA | number", "Union(DeclRef(MA) | number)"),
        ("MB | 3", "Union(DeclRef(MB) | 3)"),
        ("MA | 1", "Union(DeclRef(MA) | 1)"),
        ("MA & 1", "Intersection(DeclRef(MA) & 1)"),
    ] {
        super::checker_probe_lane_tests::with_probe(source, probe, |dispatch, node| {
            let printed = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                dispatch, node, 0,
            );
            assert_eq!(printed, written, "`{probe}` stays as written");
        });
    }
}
