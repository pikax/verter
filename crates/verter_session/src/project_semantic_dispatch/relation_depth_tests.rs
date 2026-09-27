//! The checker's two recursion rules of one relation: the depth limit —
//! the 101st nested structured relation of one `checkTypeRelatedTo` call
//! overflows, the relation is false and TS2321 is reported — and
//! `isDeeplyNestedType`, which answers `Maybe` once three instantiations of
//! one recursion identity, each newer than the last, are on both recursion
//! stacks.
//!
//! Every expected answer is TypeScript 7.0.2's, measured with
//! `tsc --ignoreConfig --noEmit --strict` under `strictNullChecks` and
//! `noImplicitAny` both on and both off (the settings agree on every probe)
//! as `const s: "q" = null! as <probe>;` read off the TS2322 message, each
//! probe in a file of its own unless a test says otherwise.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host};

/// `Box` applied `depth` times around `inner`.
fn boxed(depth: usize, inner: &str) -> String {
    format!("{}{inner}{}", "Box<".repeat(depth), ">".repeat(depth))
}

/// `{ v: … }` nested `depth` times around `inner`.
fn nested(depth: usize, inner: &str) -> String {
    format!("{}{inner}{}", "{ v: ".repeat(depth), " }".repeat(depth))
}

const BOX: &str = "interface Box<T> { v: T }\n";

/// A nesting shape: a type nested `depth` levels around an inner type.
type Shape = fn(usize, &str) -> String;

/// `[B] extends [<B's shape around number>] ? 1 : 2` over `type B` of
/// `depth` levels around `1`.
fn depth_row(shape: Shape, depth: usize) -> (String, String) {
    (
        format!("{BOX}type B = {};\n", shape(depth, "1")),
        format!("[B] extends [{}] ? 1 : 2", shape(depth, "number")),
    )
}

#[track_caller]
fn assert_rows(rows: &[(Shape, usize, &str)]) {
    let failures: Vec<String> = rows
        .iter()
        .flat_map(|&(shape, depth, expected)| {
            let (source, probe) = depth_row(shape, depth);
            mismatches(&source, &[(probe.as_str(), expected)])
                .into_iter()
                .map(move |failure| format!("depth {depth}: {failure}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Nested object types relate up to the checker's depth and overflow at it.
///
/// Measured: over `{ v: … }` nested 97, 98 and 99 times the relation is
/// `1`; nested 100, 101 and 102 times it is `2` with TS2321 (the tuple
/// wrapper is the 1st level, so 100 nested literals are the 101st).
#[test]
fn nested_object_types_overflow_at_the_checkers_depth() {
    assert_rows(&[
        (nested, 98, "1"),
        (nested, 99, "1"),
        (nested, 100, "2"),
        (nested, 101, "2"),
    ]);
}

/// Nested generic applications relate up to the checker's depth and
/// overflow at it, on the default test stack: the limit bounds the
/// relation's native recursion.
///
/// Measured: over `Box<…>` applied 90, 98 and 99 times the relation is
/// `1`; applied 100, 101 and 500 times it is `2` with TS2321.
#[test]
fn nested_generic_applications_overflow_at_the_checkers_depth() {
    assert_rows(&[
        (boxed, 90, "1"),
        (boxed, 99, "1"),
        (boxed, 100, "2"),
        (boxed, 101, "2"),
        (boxed, 500, "2"),
    ]);
}

/// A finite recursive alias over a type literal: the alias instantiates a
/// newer object type at every level, and from the third one on both
/// stacks the checker answers `Maybe`, so `1` relates to `string` there.
///
/// Measured: over `type N<T, D extends unknown[]> = D['length'] extends d
/// ? T : { v: N<T, [...D, 0]> }`, `[N<1, []>] extends [N<string, []>] ? 1
/// : 2` is `2` at `d` 1 and 2 and `1` at `d` 3, 4, 5 and 6, and
/// `[N<1, []>] extends [N<number, []>]` is `1` at every `d`.
#[test]
fn a_recursive_alias_is_deeply_nested_from_its_third_instantiation() {
    let alias = |d: usize| {
        format!("type N<T, D extends unknown[]> = D['length'] extends {d} ? T : {{ v: N<T, [...D, 0]> }};\n")
    };
    let mut failures = Vec::new();
    for (d, to_string, to_number) in [(1, "2", "1"), (2, "2", "1"), (3, "1", "1"), (6, "1", "1")] {
        for (probe, expected) in [
            ("[N<1, []>] extends [N<string, []>] ? 1 : 2", to_string),
            ("[N<1, []>] extends [N<number, []>] ? 1 : 2", to_number),
        ] {
            failures.extend(
                mismatches(&alias(d), &[(probe, expected)])
                    .into_iter()
                    .map(|failure| format!("d = {d}: {failure}")),
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An infinitely recursive alias: the recursion stops as deeply nested,
/// and a member beside the recursive one still decides the relation.
///
/// Measured: over `type L<T> = { v: L<[T]>; t: T }`, `[L<1>] extends
/// [L<number>] ? 1 : 2` is `1` and `[L<string>] extends [L<number>] ? 1 :
/// 2` is `2`.
#[test]
fn an_infinitely_recursive_alias_stops_as_deeply_nested() {
    let failures = mismatches(
        "type L<T> = { v: L<[T]>; t: T };\n",
        &[
            ("[L<1>] extends [L<number>] ? 1 : 2", "1"),
            ("[L<string>] extends [L<number>] ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A generic interface reference recurses without being deeply nested: the
/// checker relates two references to one interface by the variance of its
/// type arguments before it relates them structurally.
///
/// Measured: over `interface I<T, D extends unknown[]> { v: D['length']
/// extends 3 ? T : I<T, [...D, 0]> }`, `[I<1, []>] extends [I<string,
/// []>] ? 1 : 2` is `2` and `[I<1, []>] extends [I<number, []>] ? 1 : 2`
/// is `1`.
#[test]
fn a_recursive_interface_reference_is_not_deeply_nested() {
    let failures = mismatches(
        "interface I<T, D extends unknown[]> { v: D['length'] extends 3 ? T : I<T, [...D, 0]> }\n",
        &[
            ("[I<1, []>] extends [I<string, []>] ? 1 : 2", "2"),
            ("[I<1, []>] extends [I<number, []>] ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An infinitely recursive generic interface: the checker measures its
/// variance through the recursion, which stops as deeply nested.
///
/// Measured: over `interface R<T> { v: R<[T]>; t: T }`, `[R<1>] extends
/// [R<number>] ? 1 : 2` is `1` and `[R<string>] extends [R<number>] ? 1 :
/// 2` is `2`. This engine relates the references structurally and has no
/// recursion identity for an interface (see
/// `a_recursive_interface_reference_is_not_deeply_nested`), so the first
/// probe overflows the depth limit instead: `2`.
#[test]
#[ignore = "two references to one generic interface relate by the variance of its type arguments"]
fn an_infinitely_recursive_interface_relates_by_its_variance() {
    let failures = mismatches(
        "interface R<T> { v: R<[T]>; t: T }\n",
        &[
            ("[R<1>] extends [R<number>] ? 1 : 2", "1"),
            ("[R<string>] extends [R<number>] ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The relations an overflowed one computed are never admitted: a relation
/// checked after it answers as it does alone.
///
/// Measured in ONE file, in this order: `[Box<…99…<1>>] extends
/// [Box<…99…<number>>] ? 1 : 2` is `1`, and the same over 100 applications
/// after it is `1` too — the checker's relation cache holds the 99-deep
/// pair, so the 100-deep relation never reaches the limit. This engine's
/// memo holds the pair the same way.
#[test]
fn a_relation_after_a_shallower_one_reads_its_cached_pairs() {
    let (b99, t99) = (boxed(99, "1"), boxed(99, "number"));
    let (b100, t100) = (boxed(100, "1"), boxed(100, "number"));
    let first = format!("[{b99}] extends [{t99}] ? 1 : 2");
    let second = format!("[{b100}] extends [{t100}] ? 1 : 2");
    let failures = mismatches_in_one_host(BOX, &[(first.as_str(), "1"), (second.as_str(), "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Measured in ONE file, in this order: the 100-deep relation is `2` with
/// TS2321, and the 99-deep one after it is `2` too — the checker caches
/// the failures its overflow produced. This engine never admits an answer
/// computed under an overflow (it depends on where the relation began, and
/// the memo is shared across requests), so the 99-deep relation answers
/// `1`, as it does alone.
#[test]
#[ignore = "the relation memo keeps the failures a checker overflow produced"]
fn a_relation_after_an_overflowed_one_reads_its_failures() {
    let (b99, t99) = (boxed(99, "1"), boxed(99, "number"));
    let (b100, t100) = (boxed(100, "1"), boxed(100, "number"));
    let first = format!("[{b100}] extends [{t100}] ? 1 : 2");
    let second = format!("[{b99}] extends [{t99}] ? 1 : 2");
    let failures = mismatches_in_one_host(BOX, &[(first.as_str(), "2"), (second.as_str(), "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
