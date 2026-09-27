//! Two references to one generic interface or class relate by their type
//! arguments under the variance the checker measures for each parameter
//! (`getVariances`), or reads off its annotation.
//!
//! Every expected answer is TypeScript 7.0.2's, measured with
//! `tsc --ignoreConfig --noEmit --strict` under `strictNullChecks` and
//! `noImplicitAny` both on, both off and each alone (the settings agree on
//! every probe) as `const s: "q" = null! as <probe>;` read off the TS2322
//! message, each probe checked by a checker of its own.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host};

#[track_caller]
fn assert_answers(source: &str, rows: &[(&str, &str)]) {
    let failures = mismatches(source, rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A parameter read only as a property type is covariant.
///
/// Measured over `interface Co<T> { v: T }`: `[Co<1>] extends [Co<number>]
/// ? 1 : 2` is `1` and `[Co<number>] extends [Co<1>] ? 1 : 2` is `2`.
#[test]
fn a_covariant_parameter_relates_its_arguments_as_written() {
    assert_answers(
        "interface Co<T> { v: T }\n",
        &[
            ("[Co<1>] extends [Co<number>] ? 1 : 2", "1"),
            ("[Co<number>] extends [Co<1>] ? 1 : 2", "2"),
        ],
    );
}

/// A parameter read only as a function-typed property's parameter is
/// contravariant under `strictFunctionTypes`.
///
/// Measured over `interface Contra<T> { f: (x: T) => void }`:
/// `[Contra<number>] extends [Contra<1>] ? 1 : 2` is `1` and
/// `[Contra<1>] extends [Contra<number>] ? 1 : 2` is `2`.
#[test]
fn a_contravariant_parameter_relates_its_arguments_reversed() {
    assert_answers(
        "interface Contra<T> { f: (x: T) => void }\n",
        &[
            ("[Contra<number>] extends [Contra<1>] ? 1 : 2", "1"),
            ("[Contra<1>] extends [Contra<number>] ? 1 : 2", "2"),
        ],
    );
}

/// A parameter read only as a method's parameter is bivariant: either
/// direction relates, and unrelated arguments do not.
///
/// Measured over `interface Bi<T> { m(x: T): void }`: `[Bi<1>] extends
/// [Bi<number>]` and `[Bi<number>] extends [Bi<1>]` are `1`, `[Bi<string>]
/// extends [Bi<number>]` is `2`.
#[test]
fn a_bivariant_parameter_relates_its_arguments_either_way() {
    assert_answers(
        "interface Bi<T> { m(x: T): void }\n",
        &[
            ("[Bi<1>] extends [Bi<number>] ? 1 : 2", "1"),
            ("[Bi<number>] extends [Bi<1>] ? 1 : 2", "1"),
            ("[Bi<string>] extends [Bi<number>] ? 1 : 2", "2"),
        ],
    );
}

/// A parameter read both ways is invariant.
///
/// Measured over `interface Inv<T> { get: () => T; set: (x: T) => void }`:
/// `[Inv<1>] extends [Inv<number>] ? 1 : 2` is `2` and `[Inv<number>]
/// extends [Inv<number>] ? 1 : 2` is `1`.
#[test]
fn an_invariant_parameter_relates_only_mutually_related_arguments() {
    assert_answers(
        "interface Inv<T> { get: () => T; set: (x: T) => void }\n",
        &[
            ("[Inv<1>] extends [Inv<number>] ? 1 : 2", "2"),
            ("[Inv<number>] extends [Inv<number>] ? 1 : 2", "1"),
        ],
    );
}

/// A parameter the declaration never reads is independent: its arguments
/// are not compared.
///
/// Measured over `interface Ind<T> { n: number }`: `[Ind<string>] extends
/// [Ind<number>] ? 1 : 2` is `1`.
#[test]
fn an_independent_parameter_ignores_its_arguments() {
    assert_answers(
        "interface Ind<T> { n: number }\n",
        &[("[Ind<string>] extends [Ind<number>] ? 1 : 2", "1")],
    );
}

/// A parameter read through a `-?` mapped type is unmeasurable: arguments
/// that are not identical fall back to the structural comparison.
///
/// Measured over `interface Unm<T> { v: { [K in keyof T]-?: T[K] } }`:
/// `[Unm<{ a?: number }>] extends [Unm<{ a: number }>] ? 1 : 2` is `1`
/// (`{ a?: number }` is not assignable to `{ a: number }`, but both map to
/// `{ a: number }`), and `[Unm<{ a: number }>] extends [Unm<{ a: string }>]
/// ? 1 : 2` is `2`.
#[test]
fn an_unmeasurable_parameter_falls_back_to_structure() {
    assert_answers(
        "interface Unm<T> { v: { [K in keyof T]-?: T[K] } }\n",
        &[
            (
                "[Unm<{ a?: number }>] extends [Unm<{ a: number }>] ? 1 : 2",
                "1",
            ),
            (
                "[Unm<{ a: number }>] extends [Unm<{ a: string }>] ? 1 : 2",
                "2",
            ),
        ],
    );
}

/// A generic interface that recurses through ever-new instantiations of
/// itself is measured from its other members: its references to itself
/// answer `Unknown` while the measurement is open.
///
/// Measured: over `interface R<T> { v: R<[T]>; t: T }`, `[R<1>] extends
/// [R<number>] ? 1 : 2` is `1`, and `[R<string>] extends [R<number>]` and
/// `[R<number>] extends [R<1>]` are `2`; over `interface RC<T> { v:
/// RC<[T]>; f: (x: T) => void }`, `[RC<number>] extends [RC<1>] ? 1 : 2`
/// is `1` and `[RC<1>] extends [RC<number>] ? 1 : 2` is `2`; over
/// `interface Rec2<T> { next: Rec2<T> | null; t: T }`, `[Rec2<1>] extends
/// [Rec2<number>] ? 1 : 2` is `1` and the reverse `2`.
#[test]
fn a_recursive_generic_interface_is_measured_through_its_recursion() {
    assert_answers(
        "interface R<T> { v: R<[T]>; t: T }\n\
         interface RC<T> { v: RC<[T]>; f: (x: T) => void }\n\
         interface Rec2<T> { next: Rec2<T> | null; t: T }\n",
        &[
            ("[R<1>] extends [R<number>] ? 1 : 2", "1"),
            ("[R<string>] extends [R<number>] ? 1 : 2", "2"),
            ("[R<number>] extends [R<1>] ? 1 : 2", "2"),
            ("[RC<number>] extends [RC<1>] ? 1 : 2", "1"),
            ("[RC<1>] extends [RC<number>] ? 1 : 2", "2"),
            ("[Rec2<1>] extends [Rec2<number>] ? 1 : 2", "1"),
            ("[Rec2<number>] extends [Rec2<1>] ? 1 : 2", "2"),
        ],
    );
}

/// A class reference relates by its measured variance too, and a `void`
/// target argument of a covariant parameter falls back to structure.
///
/// Measured: over `class K<T> { constructor(public v: T) {} }`,
/// `[K<1>] extends [K<number>] ? 1 : 2` is `1` and the reverse `2`; over
/// `interface CoVoid<T> { f: () => T }`, `[CoVoid<number>] extends
/// [CoVoid<void>] ? 1 : 2` is `1`.
#[test]
fn a_class_reference_and_a_covariant_void_argument_follow_the_checker() {
    assert_answers(
        "class K<T> { constructor(public v: T) {} }\ninterface CoVoid<T> { f: () => T }\n",
        &[
            ("[K<1>] extends [K<number>] ? 1 : 2", "1"),
            ("[K<number>] extends [K<1>] ? 1 : 2", "2"),
            ("[CoVoid<number>] extends [CoVoid<void>] ? 1 : 2", "1"),
        ],
    );
}

/// A variance measurement is a relation of its own, as the checker's
/// `getVariances` starts a fresh `checkTypeRelatedTo`: measured where a
/// deep relation first needs it, it does not inherit that relation's depth.
///
/// Measured over `interface Box<T> { v: T }` and `type B = { v: …Box<1>… }`
/// with `{ v: … }` nested `n` times: `[B] extends [{ v: …Box<number>… }] ?
/// 1 : 2` is `1` at `n` 97 and 98 (the `Box` pair is the 100th nested
/// structured relation, and its arguments are simple types) and `2` with
/// TS2321 at 99.
#[test]
fn a_variance_measurement_starts_a_relation_chain_of_its_own() {
    let nested = |depth: usize, inner: &str| {
        format!("{}{inner}{}", "{ v: ".repeat(depth), " }".repeat(depth))
    };
    let failures: Vec<String> = [(97, "1"), (98, "1"), (99, "2")]
        .into_iter()
        .flat_map(|(depth, expected)| {
            let source = format!(
                "interface Box<T> {{ v: T }}\ntype B = {};\n",
                nested(depth, "Box<1>")
            );
            let probe = format!("[B] extends [{}] ? 1 : 2", nested(depth, "Box<number>"));
            mismatches(&source, &[(probe.as_str(), expected)])
                .into_iter()
                .map(move |failure| format!("depth {depth}: {failure}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Two declarations whose variances depend on each other: the one measured
/// inside the other's measurement answers the outer declaration's
/// references `Unknown` while it is open, so what it measures there is not
/// its own variance and is never memoized. Each relation measures what it
/// needs as if it were the first, and one snapshot answers alike in either
/// query order.
///
/// Measured over `interface E<T> { d: D<T> }` and `interface D<T> { e:
/// E<T>; f: (x: T) => void }`: `[D<1>] extends [D<number>] ? 1 : 2` is
/// `2`, `[E<1>] extends [E<number>] ? 1 : 2` is `2` and `[E<number>]
/// extends [E<1>] ? 1 : 2` is `1` (`E` is contravariant through `D`), each
/// in a file of its own and in one file in either order. Measured inside
/// `D`'s measurement, `E` reads independent.
#[test]
fn mutually_dependent_variances_answer_alike_in_either_order() {
    let source = "interface E<T> { d: D<T> }\ninterface D<T> { e: E<T>; f: (x: T) => void }\n";
    let d = ("[D<1>] extends [D<number>] ? 1 : 2", "2");
    let e = ("[E<1>] extends [E<number>] ? 1 : 2", "2");
    let reversed = ("[E<number>] extends [E<1>] ? 1 : 2", "1");
    let mut failures = mismatches(source, &[d, e, reversed]);
    for rows in [[d, e, reversed], [e, reversed, d]] {
        failures.extend(mismatches_in_one_host(source, &rows));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
