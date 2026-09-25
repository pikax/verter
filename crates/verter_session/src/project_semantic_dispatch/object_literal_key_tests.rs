//! Computed keys of an object literal in a function body. A key whose
//! value is a literal — read off a local as readily as written in place —
//! names one property; a key of an open `string`, `number` or `symbol`
//! type is late-bound: it names no property and contributes to the
//! literal's implicit index signature of its kind
//! (`getObjectLiteralIndexInfo`), whose value also unions every named
//! member that kind applies to.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `declare const v: <probe>; export const s: null = v;` read
//! off the TS2322 message (`tsc --noEmit --strict --ignoreConfig`). The
//! checker prints a late-bound member by its authored name (`{ [s]: number;
//! a: string; }`), so the index signatures are read off the literal's
//! surface and compared with what the measured `keyof` and indexed reads
//! imply (an indexed read through an index signature is not a projection
//! this lane answers for any declaration).

use super::checker_probe_lane_tests::{mismatches, with_probe};
use crate::semantic_query::SemanticNodeData;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};

const FIXTURE: &str = "\
declare const s: string;
declare const n: number;
declare const sym: symbol;
export function c1() { const k = 'key'; return { [k]: 1 }; }
export function c2() { let k = 'key'; return { [k]: 1 }; }
export function c3() { const k = 2; return { [k]: 'two', a: true }; }
export function c4() { return { a: 'x', [s]: 1 }; }
export function c5() { return { [n]: 'x', 1: true, a: 1 }; }
export function c6() { return { [s]: 1, [n]: true }; }
export function c7(p: string) { return { [p]: 1, b: true }; }
export function c8() { return { m() { return 1; }, [s]: 'x' }; }
export function c9() { return { [sym]: 1, a: 'x' }; }
export function c10(p: symbol) { return { [p]: 1 }; }
export function c11() { const k = 'key' as const; const o = { [k]: 1 }; return o.key; }
";

/// A key read off a `const` local holding a literal names that property.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof c1>` is `{ key:
/// number; }`; `keyof ReturnType<typeof c3>` is `"a" | 2` and its `[2]`
/// read is `string`; `ReturnType<typeof c11>` is `number`. (`keyof` over a
/// named application stays symbolic at this lane's altitude, so the key set
/// is read off the surface.)
#[test]
fn a_computed_key_read_off_a_literal_local_names_its_property() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof c1>", "{ key: number; }"),
            ("ReturnType<typeof c3>[2]", "string"),
            ("ReturnType<typeof c11>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_late_bound("ReturnType<typeof c3>", &["2", "a"], &[]);
}

/// The members a late-bound literal names and the index signatures it
/// carries, as `(key, value)` checker prints.
#[track_caller]
fn assert_late_bound(probe: &str, members: &[&str], index: &[(&str, &str)]) {
    with_probe(FIXTURE, probe, |dispatch, node| {
        let Some(SemanticNodeData::Object(view)) =
            dispatch.graph().node_data(node).as_deref().cloned()
        else {
            panic!(
                "{probe}: expected an object, measured `{}`",
                render_node(dispatch, node, 0)
            );
        };
        let mut named: Vec<String> = view
            .positive_members()
            .iter()
            .map(|member| {
                member
                    .key
                    .published_name()
                    .map_or_else(|| format!("{:?}", member.key), |name| name.to_string())
            })
            .collect();
        named.sort();
        assert_eq!(named, members, "{probe}: named members");
        assert_eq!(
            view.index_signatures.len(),
            index.len(),
            "{probe}: index signatures"
        );
        for (signature, (key, value)) in view.index_signatures.iter().zip(index) {
            for (node, print) in [(signature.key_type, key), (signature.value_type, value)] {
                let parsed = checker_syntax::parse(print).expect("checker print parses");
                assert!(
                    checker_syntax::matches_node(dispatch, node, &parsed, 0),
                    "{probe}: expected `{print}`, measured `{}`",
                    render_node(dispatch, node, 0)
                );
            }
        }
    });
}

/// A key of an open type is late-bound to the index signature of its kind.
///
/// Measured on TypeScript 7.0.2: `keyof ReturnType<typeof c2>` is `string |
/// number` and its `[string]` read is `number`; `keyof ReturnType<typeof
/// c4>` is `string | number` and its `[string]` read `string | number`;
/// `keyof ReturnType<typeof c5>` is `number | "a"`, its `[number]` read
/// `string | boolean` and its `['a']` read `number`; `ReturnType<typeof
/// c6>` reads `number | boolean` at `[string]` and `boolean` at `[number]`
/// (`keyof` is `string | number`); `ReturnType<typeof c7>` reads `number |
/// boolean` at `[string]` and `boolean` at `['b']`; `ReturnType<typeof c8>`
/// reads `string | (() => number)` at `[string]` (`keyof` is `string |
/// number`); `keyof ReturnType<typeof c9>` is `symbol | "a"` and its
/// `[symbol]` read is `number`; `keyof ReturnType<typeof c10>` is `symbol`.
#[test]
fn an_open_computed_key_is_late_bound_to_an_index_signature() {
    assert_late_bound("ReturnType<typeof c2>", &[], &[("string", "number")]);
    assert_late_bound(
        "ReturnType<typeof c4>",
        &["a"],
        &[("string", "string | number")],
    );
    assert_late_bound(
        "ReturnType<typeof c5>",
        &["1", "a"],
        &[("number", "string | boolean")],
    );
    assert_late_bound(
        "ReturnType<typeof c6>",
        &[],
        &[("string", "number | boolean"), ("number", "boolean")],
    );
    assert_late_bound(
        "ReturnType<typeof c7>",
        &["b"],
        &[("string", "number | boolean")],
    );
    assert_late_bound(
        "ReturnType<typeof c8>",
        &["m"],
        &[("string", "string | (() => number)")],
    );
    assert_late_bound("ReturnType<typeof c9>", &["a"], &[("symbol", "number")]);
    assert_late_bound("ReturnType<typeof c10>", &[], &[("symbol", "number")]);
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof c5>['a']", "number"),
            ("ReturnType<typeof c7>['b']", "boolean"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
