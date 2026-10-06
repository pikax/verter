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

/// Each type parameter's variance is its own: an annotated parameter keeps
/// its annotation beside an unannotated one, whose variance is measured.
///
/// Measured: over `interface Mixed<in out T, U> { value: T; other: U }`,
/// `[Mixed<1, string>] extends [Mixed<number, string>] ? 1 : 2` is `2`
/// (`T` is invariant), `[Mixed<number, 'a'>] extends [Mixed<number,
/// string>]` is `1` and `[Mixed<number, string>] extends [Mixed<number,
/// 'a'>]` is `2` (`U` is measured covariant); over `interface MixedIn<in
/// T, U> { value: (x: T) => void; other: U }`, `[MixedIn<number, 'a'>]
/// extends [MixedIn<1, string>]` is `1` and `[MixedIn<1, 'a'>] extends
/// [MixedIn<number, string>]` is `2`; over `type MixedAlias<in out T, U> =
/// { value: T; other: U }`, `[MixedAlias<1, string>] extends
/// [MixedAlias<number, string>]` is `2` and `[MixedAlias<number, 'a'>]
/// extends [MixedAlias<number, string>]` is `1`; over `interface
/// MixedRec<in out T, U> { value: T; next: MixedRec<T, [U]>; other: U }`,
/// `[MixedRec<number, 'a'>] extends [MixedRec<number, string>]` is `1` and
/// `[MixedRec<1, 'a'>] extends [MixedRec<number, string>]` is `2`.
#[test]
fn an_annotated_parameter_keeps_its_variance_beside_a_measured_one() {
    assert_answers(
        "interface Mixed<in out T, U> { value: T; other: U }\n\
         interface MixedIn<in T, U> { value: (x: T) => void; other: U }\n\
         type MixedAlias<in out T, U> = { value: T; other: U };\n\
         interface MixedRec<in out T, U> { value: T; next: MixedRec<T, [U]>; other: U }\n",
        &[
            (
                "[Mixed<1, string>] extends [Mixed<number, string>] ? 1 : 2",
                "2",
            ),
            (
                "[Mixed<number, 'a'>] extends [Mixed<number, string>] ? 1 : 2",
                "1",
            ),
            (
                "[Mixed<number, string>] extends [Mixed<number, 'a'>] ? 1 : 2",
                "2",
            ),
            (
                "[MixedIn<number, 'a'>] extends [MixedIn<1, string>] ? 1 : 2",
                "1",
            ),
            (
                "[MixedIn<1, 'a'>] extends [MixedIn<number, string>] ? 1 : 2",
                "2",
            ),
            (
                "[MixedAlias<1, string>] extends [MixedAlias<number, string>] ? 1 : 2",
                "2",
            ),
            (
                "[MixedAlias<number, 'a'>] extends [MixedAlias<number, string>] ? 1 : 2",
                "1",
            ),
            (
                "[MixedRec<number, 'a'>] extends [MixedRec<number, string>] ? 1 : 2",
                "1",
            ),
            (
                "[MixedRec<1, 'a'>] extends [MixedRec<number, string>] ? 1 : 2",
                "2",
            ),
        ],
    );
}

/// A parameter whose measurement reaches a rest parameter, a template
/// literal or a mapped type's constraint is unreliable (`VarianceFlags`
/// `Unreliable`): when its arguments do not relate by the variance, the
/// references fall back to the structural comparison.
///
/// Measured: over `interface H<T extends unknown[]> { f: (...args: T) =>
/// void }`, `[H<[]>] extends [H<[number]>] ? 1 : 2` is `1` (`() => void`
/// takes the place of `(x: number) => void`, though `[number]` is not
/// assignable to `[]`) and `[H<[number]>] extends [H<[]>]` and
/// `[H<[1]>] extends [H<[number]>]` are `2`; the method form `interface
/// HM<T extends unknown[]> { f(...args: T): void }` answers `[HM<[]>]
/// extends [HM<[number]>]` `1`; over `interface TN<T extends string |
/// number> { v: `${T}` }`, `[TN<'1'>] extends [TN<1>]` and `[TN<1>]
/// extends [TN<'1'>]` are `1` (both read `"1"`) and `[TN<'x'>] extends
/// [TN<string>]` is `1`; over `interface MO<T> { v: { [K in keyof T]?:
/// T[K] } }`, `[MO<{ a: 1; b: 2 }>] extends [MO<{ a: number }>]` and
/// `[MO<{ a: number }>] extends [MO<{ a: number; b: 2 }>]` are `1`.
#[test]
fn an_unreliable_parameter_falls_back_to_structure() {
    let source = "interface H<T extends unknown[]> { f: (...args: T) => void }\n\
         interface HM<T extends unknown[]> { f(...args: T): void }\n\
         interface TN<T extends string | number> { v: `${T}` }\n\
         interface MO<T> { v: { [K in keyof T]?: T[K] } }\n";
    let rows = [
        ("[H<[]>] extends [H<[number]>] ? 1 : 2", "1"),
        ("[H<[number]>] extends [H<[]>] ? 1 : 2", "2"),
        ("[H<[1]>] extends [H<[number]>] ? 1 : 2", "2"),
        ("[HM<[]>] extends [HM<[number]>] ? 1 : 2", "1"),
        ("[TN<'1'>] extends [TN<1>] ? 1 : 2", "1"),
        ("[TN<1>] extends [TN<'1'>] ? 1 : 2", "1"),
        ("[TN<'x'>] extends [TN<string>] ? 1 : 2", "1"),
        (
            "[MO<{ a: 1; b: 2 }>] extends [MO<{ a: number }>] ? 1 : 2",
            "1",
        ),
        (
            "[MO<{ a: number }>] extends [MO<{ a: number; b: 2 }>] ? 1 : 2",
            "1",
        ),
    ];
    let mut failures = mismatches(source, &rows);
    // A measurement read back from the memo keeps its report.
    let reversed: Vec<(&str, &str)> = rows.iter().rev().copied().collect();
    for rows in [&rows[..], &reversed[..]] {
        failures.extend(mismatches_in_one_host(source, rows));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A reliable variance decides on its own, with no structural fallback.
///
/// Measured: over `interface HR<T> { f: (...args: T[]) => void }`,
/// `[HR<number>] extends [HR<1>] ? 1 : 2` is `1` and `[HR<1>] extends
/// [HR<number>]` is `2`; over `interface TA<T extends string> { v:
/// `a${T}` }`, `[TA<'x'>] extends [TA<string>]` is `1` and `[TA<string>]
/// extends [TA<'x'>]` is `2`; over `interface MP<T> { v: { [K in keyof
/// T]: T[K] } }`, `[MP<{ a: 1 }>] extends [MP<{ a: number }>]` is `1` and
/// the reverse `2`.
#[test]
fn rest_template_and_mapped_parameters_keep_their_variance() {
    assert_answers(
        "interface HR<T> { f: (...args: T[]) => void }\n\
         interface TA<T extends string> { v: `a${T}` }\n\
         interface MP<T> { v: { [K in keyof T]: T[K] } }\n",
        &[
            ("[HR<number>] extends [HR<1>] ? 1 : 2", "1"),
            ("[HR<1>] extends [HR<number>] ? 1 : 2", "2"),
            ("[TA<'x'>] extends [TA<string>] ? 1 : 2", "1"),
            ("[TA<string>] extends [TA<'x'>] ? 1 : 2", "2"),
            ("[MP<{ a: 1 }>] extends [MP<{ a: number }>] ? 1 : 2", "1"),
            ("[MP<{ a: number }>] extends [MP<{ a: 1 }>] ? 1 : 2", "2"),
        ],
    );
}

/// The variance markers and marker instantiations a measurement interns in
/// the semantic graph, with the scope each was interned under.
fn variance_marker_nodes(
    dispatch: &super::ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
) -> Vec<(
    verter_type_engine::semantic_query::SemanticNodeId,
    verter_type_engine::semantic_query::DeclIdentity,
    Option<verter_type_engine::semantic_query::NodeScopeId>,
)> {
    use verter_type_engine::semantic_query::{SemanticNodeData, SemanticNodeId};
    let graph = dispatch.graph();
    (0..graph.node_count() as u64)
        .map(SemanticNodeId)
        .filter_map(|node| {
            let declaration = match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::InstantiationRef { base, args })
                    if args
                        .iter()
                        .any(|arg| dispatch.variance_marker_of(*arg).is_some()) =>
                {
                    base.clone()
                }
                _ => dispatch.variance_marker_of(node)?.0,
            };
            Some((node, declaration, graph.node_scope(node)))
        })
        .collect()
}

/// A measurement's markers and marker instantiations are nodes of the
/// measured declaration's own file generation — at most six per type
/// parameter — so they are released with that generation, and measuring
/// again on the same generation interns none. An edit of the declaring
/// file measures under the new generation, never beside the old one.
///
/// Oracle: over `interface Co<T> { v: T }` and, after the edit, `interface
/// Co<T> { v: T; w: number }`, `[Co<1>] extends [Co<number>] ? 1 : 2` is
/// `1`.
#[test]
fn variance_markers_belong_to_their_declarations_file_generation() {
    use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host};
    let host = default_probe_host();
    let probe = "[Co<1>] extends [Co<number>] ? 1 : 2";
    let measure = |source: &str| {
        with_probe_on_host(
            &host,
            Default::default(),
            source,
            probe,
            |dispatch, node| {
                let answer = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                    dispatch, node, 0,
                );
                assert_eq!(answer, "1", "over `{source}`");
                variance_marker_nodes(dispatch)
            },
        )
    };
    let first = measure("interface Co<T> { v: T }\n");
    assert!(
        (1..=6).contains(&first.len()),
        "one parameter's measurement interns at most six marker nodes: {first:?}"
    );
    for (node, declaration, scope) in &first {
        assert_eq!(
            scope.as_ref(),
            Some(&super::relation_variance::declaration_scope(declaration)),
            "marker node {node:?} is scoped to its declaration's file generation"
        );
    }
    assert_eq!(
        measure("interface Co<T> { v: T }\n"),
        first,
        "measuring again on the same generation interns no marker"
    );
    let edited = measure("interface Co<T> { v: T; w: number }\n");
    let first_hash = first[0].1.whole_hash;
    let fresh: Vec<_> = edited
        .iter()
        .filter(|(_, declaration, _)| declaration.whole_hash != first_hash)
        .collect();
    assert!(
        !fresh.is_empty() && fresh.len() <= 6,
        "the edited declaration is measured under its new generation: {edited:?}"
    );
    assert_eq!(
        edited.len() - fresh.len(),
        first.len(),
        "no marker of the old generation is interned after the edit"
    );
}

/// Closing the declaring document ([`crate::VerterHost::evict`]) releases
/// a measurement's markers and marker instantiations with the rest of its
/// nodes (the close cascade's tombstone) — those of every generation an
/// edit superseded as well as the current one's — and relating after the
/// reload answers alike without reading a released node. An edit itself
/// releases no payload (it drains the memo and the arena's dedup entries,
/// so a superseded generation's markers are never read or interned
/// again); the close does.
///
/// Oracle: over `interface Co<T> { v: T }` and, after the edit,
/// `interface Co<T> { v: T; w: number }`, `[Co<1>] extends [Co<number>] ?
/// 1 : 2` is `1`, before and after the close.
#[test]
fn closing_the_declaring_document_releases_its_variance_markers() {
    use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host, PROBE_FILE};
    let host = default_probe_host();
    let probe = "[Co<1>] extends [Co<number>] ? 1 : 2";
    let measure = |source: &str| {
        with_probe_on_host(
            &host,
            Default::default(),
            source,
            probe,
            |dispatch, node| {
                let answer = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                    dispatch, node, 0,
                );
                assert_eq!(answer, "1", "over `{source}`");
                variance_marker_nodes(dispatch)
            },
        )
    };
    let first = "interface Co<T> { v: T }\n";
    let markers = measure(first);
    let edited = measure("interface Co<T> { v: T; w: number }\n");
    assert!(
        edited.len() > markers.len(),
        "the edit measured under a new generation: {markers:?} then {edited:?}"
    );
    let graph = std::sync::Arc::clone(host.project_type_store().semantic_graph());
    assert!(edited.iter().all(|(node, _, _)| graph.node_is_live(*node)));
    // The editor closes the document: the host evicts it, and WSP6 applies
    // the close-time release as soon as no computation is in flight.
    host.evict(PROBE_FILE);
    let store = host.project_type_store();
    store.try_apply_deferred_releases();
    assert_eq!(
        store.deferred_release_count(),
        0,
        "the close release applied"
    );
    let live: Vec<_> = edited
        .iter()
        .filter(|(node, _, _)| graph.node_is_live(*node))
        .collect();
    assert!(
        live.is_empty(),
        "closing {PROBE_FILE} releases every marker node of both generations; still live: {live:?}"
    );
    // The reload answers alike, and a released marker never reads live
    // again: whatever the reload measures, it measures under fresh nodes.
    let remeasured = measure(first);
    assert!(
        remeasured
            .iter()
            .filter(|(node, _, _)| graph.node_is_live(*node))
            .all(|(node, _, _)| !edited.iter().any(|(old, _, _)| old == node)),
        "the reloaded document reads no released marker: {remeasured:?}"
    );
    assert!(edited.iter().all(|(node, _, _)| !graph.node_is_live(*node)));
}
