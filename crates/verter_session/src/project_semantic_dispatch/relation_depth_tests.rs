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

/// A relation answers what it answers cold, whatever the relations checked
/// before it left in the memo: the answer of a fixed demand never depends
/// on query order. The checker's own relation cache does make it depend:
/// its answers in ONE file, in order, are what the rows below call warm.
///
/// Measured: `[Box<…99…<1>>] extends [Box<…99…<number>>] ? 1 : 2` is `1`
/// and the 100-deep relation is `2` with TS2321, each in a file (and a
/// checker) of its own. In one file, the 99-deep relation first makes the
/// 100-deep one `1` (the cached 99-deep pair spares it the depth that
/// overflows it), and the 100-deep relation first makes the 99-deep one
/// `2` (the failures its overflow cached). Here both orders answer the
/// cold answers: an entry is replayed inside a relation only where its
/// recorded height fits the depth left.
#[test]
fn a_relation_answers_its_cold_answer_in_either_order_around_the_depth_limit() {
    let (b99, t99) = (boxed(99, "1"), boxed(99, "number"));
    let (b100, t100) = (boxed(100, "1"), boxed(100, "number"));
    let shallow = format!("[{b99}] extends [{t99}] ? 1 : 2");
    let deep = format!("[{b100}] extends [{t100}] ? 1 : 2");
    let mut failures = Vec::new();
    for rows in [
        [(shallow.as_str(), "1"), (deep.as_str(), "2")],
        [(deep.as_str(), "2"), (shallow.as_str(), "1")],
    ] {
        failures.extend(mismatches_in_one_host(BOX, &rows));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const NESTED_ALIAS: &str =
    "type N<T, D extends unknown[]> = D['length'] extends 3 ? T : { v: N<T, [...D, 0]> };\n";

/// A relation deeply nested cold is deeply nested after a relation whose
/// memo entries sit inside its recursion, and one that is not stays so.
///
/// Measured over [`NESTED_ALIAS`], each probe in a file of its own:
/// `[N<1, [0, 0]>] extends [N<string, [0, 0]>] ? 1 : 2` is `2` (two
/// instantiations of `N` before its body answers `T`), and `[N<1, []>]
/// extends [N<string, []>] ? 1 : 2` is `1` (the third instantiation is
/// deeply nested). In one file the checker answers the second relation
/// `2` after the first, and the first `1` after the second. Here both
/// orders answer the cold answers: an entry is replayed only where the
/// instantiations it stacks, with those below the read, cannot complete a
/// deeply-nested stack.
#[test]
fn a_relation_answers_its_cold_answer_in_either_order_around_a_deeply_nested_stack() {
    let deep = "[N<1, [0, 0]>] extends [N<string, [0, 0]>] ? 1 : 2";
    let shallow = "[N<1, []>] extends [N<string, []>] ? 1 : 2";
    let mut failures = Vec::new();
    for rows in [[(deep, "2"), (shallow, "1")], [(shallow, "1"), (deep, "2")]] {
        failures.extend(mismatches_in_one_host(NESTED_ALIAS, &rows));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// One snapshot answers every query order alike: the same probes, read
/// from one host forward, backward and interleaved, each answer its cold
/// answer.
///
/// Measured, each probe in a file of its own: over `Box` applied 98, 99,
/// 100 and 101 times the relation is `1`, `1`, `2`, `2`; over
/// [`NESTED_ALIAS`], `[N<1, D>] extends [N<string, D>]` is `2` for `D` of
/// length 2 and 1 and `1` for `[]`.
#[test]
fn one_snapshot_answers_every_query_order_alike() {
    let source = format!("{BOX}{NESTED_ALIAS}");
    let box_rows: Vec<(String, &str)> = [(98, "1"), (99, "1"), (100, "2"), (101, "2")]
        .into_iter()
        .map(|(depth, answer)| {
            (
                format!(
                    "[{}] extends [{}] ? 1 : 2",
                    boxed(depth, "1"),
                    boxed(depth, "number")
                ),
                answer,
            )
        })
        .collect();
    let alias_rows = [
        (
            "[N<1, [0, 0]>] extends [N<string, [0, 0]>] ? 1 : 2".to_owned(),
            "2",
        ),
        (
            "[N<1, [0]>] extends [N<string, [0]>] ? 1 : 2".to_owned(),
            "2",
        ),
        ("[N<1, []>] extends [N<string, []>] ? 1 : 2".to_owned(), "1"),
    ];
    let rows: Vec<(&str, &str)> = box_rows
        .iter()
        .chain(alias_rows.iter())
        .map(|(probe, answer)| (probe.as_str(), *answer))
        .collect();
    let backward: Vec<(&str, &str)> = rows.iter().rev().copied().collect();
    let interleaved: Vec<(&str, &str)> = rows
        .iter()
        .step_by(2)
        .chain(rows.iter().skip(1).step_by(2))
        .copied()
        .collect();
    let mut failures = Vec::new();
    for (order, rows) in [
        ("forward", &rows),
        ("backward", &backward),
        ("interleaved", &interleaved),
    ] {
        failures.extend(
            mismatches_in_one_host(&source, rows)
                .into_iter()
                .map(|failure| format!("{order}: {failure}")),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
