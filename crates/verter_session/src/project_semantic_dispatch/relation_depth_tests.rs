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

/// An infinitely recursive generic interface relates by the variance the
/// checker measures through its recursion: its references to itself answer
/// `Unknown` while the measurement is open, so it never descends a level
/// per nesting.
///
/// Measured: over `interface R<T> { v: R<[T]>; t: T }`, `[R<1>] extends
/// [R<number>] ? 1 : 2` is `1` and `[R<string>] extends [R<number>] ? 1 :
/// 2` is `2`.
#[test]
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

/// Written inline on both sides, nested object types overflow at the same
/// depth as through an alias: the tuple's element pair is the checker's
/// second nested structured relation.
///
/// Measured: `[{ v: …99…1… }] extends [{ v: …99…number… }] ? 1 : 2` is `1`
/// and over 100 nested literals it is `2` with TS2321. This engine relates
/// a tuple's or an array's element pair of two object literal types inside
/// the enclosing relation's frame, so that pair never counts toward the
/// depth: the 100-deep relation answers `1`.
#[test]
fn inline_nested_object_types_overflow_at_the_checkers_depth() {
    let failures: Vec<String> = [(99, "1"), (100, "2")]
        .into_iter()
        .flat_map(|(depth, expected)| {
            let probe = format!(
                "[{}] extends [{}] ? 1 : 2",
                nested(depth, "1"),
                nested(depth, "number")
            );
            mismatches("", &[(probe.as_str(), expected)])
                .into_iter()
                .map(move |failure| format!("depth {depth}: {failure}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `[S] extends [<S's shape around number>] ? 1 : 2` for a shape around `1`
/// nested `depth` times, written inline on both sides.
fn inline_row(open: &str, close: &str, depth: usize) -> (String, String) {
    let wrap = |inner: &str| format!("{}{inner}{}", open.repeat(depth), close.repeat(depth));
    (
        format!("[{}] extends [{}] ? 1 : 2", wrap("1"), wrap("number")),
        format!("{open}…{close} × {depth}"),
    )
}

#[track_caller]
fn assert_inline_rows(
    project: super::checker_probe_lane_tests::ProbeProject<'_>,
    rows: &[(&str, &str, usize, &str)],
) {
    let failures: Vec<String> = rows
        .iter()
        .flat_map(|&(open, close, depth, expected)| {
            let (probe, label) = inline_row(open, close, depth);
            super::checker_probe_lane_tests::mismatches_in(
                project,
                "",
                &[(probe.as_str(), expected)],
            )
            .into_iter()
            .map(move |failure| format!("{label}: {failure}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every pair the checker relates with an `isRelatedTo` of its own is one
/// entry of its recursion depth, whether a relation frame of this engine
/// or its inline worklist relates it: an element pair of two tuples or two
/// arrays, and a union pair the checker caches.
///
/// Measured (all four settings agree unless noted): with the tuple wrapper
/// as the first entry, `[[{ v: …n…1… }]]`-style `[{ v: … }]` nested 48 and
/// 49 times relates (`1`) and 50 times overflows (`2`, TS2321) — two
/// entries a level; `{ v: … }[]` nested 49 times relates and 50 and 51
/// overflow; tuples `[…]` nested 98 and 99 times relate and 100
/// overflow; `{ v: … } | null` nested 49 times relates and 50 overflows
/// under `strictNullChecks` (the union pair and the object pair each
/// count; the member-to-union step, `typeRelatedToSomeType`, does not);
/// `{ v: … } | null | undefined | 0` nested 33 and 49 times relates and 50
/// overflows.
#[test]
fn inline_element_and_union_pairs_count_toward_the_checkers_depth() {
    assert_inline_rows(
        Default::default(),
        &[
            ("[{ v: ", " }]", 48, "1"),
            ("[{ v: ", " }]", 49, "1"),
            ("[{ v: ", " }]", 50, "2"),
            ("{ v: ", " }[]", 49, "1"),
            ("{ v: ", " }[]", 50, "2"),
            ("{ v: ", " }[]", 51, "2"),
            ("[", "]", 99, "1"),
            ("[", "]", 100, "2"),
            ("{ v: ", " } | null", 49, "1"),
            ("{ v: ", " } | null", 50, "2"),
            ("{ v: ", " } | null | undefined | 0", 33, "1"),
            ("{ v: ", " } | null | undefined | 0", 49, "1"),
            ("{ v: ", " } | null | undefined | 0", 50, "2"),
        ],
    );
}

/// Without `strictNullChecks` the checker's union of an object type and
/// `null` is the object type alone, one entry a level.
///
/// Measured with `strictNullChecks` off (`noImplicitAny` on and off):
/// `{ v: … } | null` nested 50 and 99 times relates (`1`) and 100 times
/// overflows (`2`).
#[test]
fn a_union_erased_without_strict_null_checks_counts_as_its_object() {
    assert_inline_rows(
        super::checker_probe_lane_tests::ProbeProject {
            compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
            ..Default::default()
        },
        &[
            ("{ v: ", " } | null", 50, "1"),
            ("{ v: ", " } | null", 99, "1"),
            ("{ v: ", " } | null", 100, "2"),
        ],
    );
}

/// Relating two wide tuples of object pairs costs the same relation checks
/// and structural reductions per element at 500, 1,000 and 2,000
/// elements: an element pair's inline recursion entry is a counter on its
/// frame, not a relation of its own.
///
/// Oracle: the relation holds, `1`, at every width.
#[test]
fn relating_wide_tuples_of_object_pairs_costs_the_same_per_element() {
    let elements = |width: usize, value: &str| {
        (0..width)
            .map(|index| format!("{{ a{index}: {value} }}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let work = |width: usize| {
        let probe = format!(
            "[{}] extends [{}] ? 1 : 2",
            elements(width, "1"),
            elements(width, "number")
        );
        let reductions = super::ProjectSemanticDispatch::relation_reductions_for_tests();
        let (checks, answer) =
            super::checker_probe_lane_tests::with_probe("", &probe, |dispatch, node| {
                (
                    dispatch.graph().stats_snapshot().relation_check_count,
                    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                        dispatch, node, 0,
                    ),
                )
            });
        assert_eq!(answer, "1", "the {width}-element relation holds");
        (
            checks,
            super::ProjectSemanticDispatch::relation_reductions_for_tests() - reductions,
        )
    };
    let (at_500, at_1000, at_2000) = (work(500), work(1000), work(2000));
    assert_eq!(
        (at_1000.0 - at_500.0) * 2,
        at_2000.0 - at_1000.0,
        "relation checks at 500 / 1,000 / 2,000 elements: {at_500:?} / {at_1000:?} / {at_2000:?}"
    );
    assert_eq!(
        (at_1000.1 - at_500.1) * 2,
        at_2000.1 - at_1000.1,
        "reductions at 500 / 1,000 / 2,000 elements: {at_500:?} / {at_1000:?} / {at_2000:?}"
    );
}

/// A union source of fewer than four members against a target that is no
/// union is no recursion entry of its own (`skipCaching`): only each
/// member's relation to the target is.
///
/// Measured over `type S0 = 1; type T0 = number;` and for `i` from 1 to
/// `n`, `type Si = { v: S(i-1) } | Ti;` and `type Ti = { v: T(i-1) };`:
/// `[Sn] extends [Tn] ? 1 : 2` is `1` at `n` 49, 50, 98 and 99 and `2`
/// with TS2321 at 100 — one entry a level after the tuple's.
#[test]
fn a_small_union_source_is_no_recursion_entry_of_its_own() {
    let failures: Vec<String> = [(50, "1"), (99, "1"), (100, "2")]
        .into_iter()
        .flat_map(|(depth, expected)| {
            let mut source = String::from("type S0 = 1;\ntype T0 = number;\n");
            for level in 1..=depth {
                source.push_str(&format!(
                    "type S{level} = {{ v: S{} }} | T{level};\ntype T{level} = {{ v: T{} }};\n",
                    level - 1,
                    level - 1
                ));
            }
            let probe = format!("[S{depth}] extends [T{depth}] ? 1 : 2");
            mismatches(&source, &[(probe.as_str(), expected)])
                .into_iter()
                .map(move |failure| format!("depth {depth}: {failure}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Relating two wide object types looks each target property up on the
/// source without scanning it: the member comparisons key lookups make by
/// scanning a surface grow linearly with the width at 500, 1,000 and 2,000
/// members, as the relation checks do.
///
/// Oracle: `[{ k0: { a: 1 }, … }] extends [{ k0: { a: number }, … }] ? 1 :
/// 2` holds, `1`, at every width.
#[test]
fn relating_wide_object_types_costs_the_same_per_member() {
    let members = |width: usize, value: &str| {
        (0..width)
            .map(|index| format!("k{index}: {{ a: {value} }}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let work = |width: usize| {
        let probe = format!(
            "[{{ {} }}] extends [{{ {} }}] ? 1 : 2",
            members(width, "1"),
            members(width, "number")
        );
        let scans = crate::semantic_query::key_scan_comparisons_for_tests();
        let (checks, answer) =
            super::checker_probe_lane_tests::with_probe("", &probe, |dispatch, node| {
                (
                    dispatch.graph().stats_snapshot().relation_check_count,
                    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                        dispatch, node, 0,
                    ),
                )
            });
        assert_eq!(answer, "1", "the {width}-member relation holds");
        (
            checks,
            crate::semantic_query::key_scan_comparisons_for_tests() - scans,
        )
    };
    let (at_500, at_1000, at_2000) = (work(500), work(1000), work(2000));
    assert_eq!(
        (at_1000.0 - at_500.0) * 2,
        at_2000.0 - at_1000.0,
        "relation checks at 500 / 1,000 / 2,000 members: {at_500:?} / {at_1000:?} / {at_2000:?}"
    );
    assert_eq!(
        (at_1000.1 - at_500.1) * 2,
        at_2000.1 - at_1000.1,
        "scanned key comparisons at 500 / 1,000 / 2,000 members: {at_500:?} / {at_1000:?} / {at_2000:?}"
    );
}

/// A target property finds the source member addressing the same JS
/// property, whichever spelling each writes.
///
/// Measured: `[{ 1: 1 }] extends [{ "1": number }] ? 1 : 2`, `[{ "1": 1 }]
/// extends [{ 1: number }]` and `[{ 1: 1; b: 2 }] extends [{ "1": number;
/// b: number }]` are `1`; `[{ "01": 1 }] extends [{ 1: number }]` is `2`.
#[test]
fn a_property_finds_the_member_of_either_spelling() {
    let failures = mismatches(
        "",
        &[
            (r#"[{ 1: 1 }] extends [{ "1": number }] ? 1 : 2"#, "1"),
            (r#"[{ "1": 1 }] extends [{ 1: number }] ? 1 : 2"#, "1"),
            (
                r#"[{ 1: 1; b: 2 }] extends [{ "1": number; b: number }] ? 1 : 2"#,
                "1",
            ),
            (r#"[{ "01": 1 }] extends [{ 1: number }] ? 1 : 2"#, "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
