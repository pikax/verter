//! An equality with a VALUE that is not a literal narrows the compared
//! binding by the value's type through the comparable relation, as the
//! checker's `narrowTypeByEquality` does: the positive edge keeps the
//! arms comparable to the value's type, a class instance whose private or
//! protected member makes it incomparable included, and the negated edge
//! keeps the declared type.
//!
//! Every expected print is TypeScript 7.0.2's, measured on the fixture
//! with `tsc --ignoreConfig --declaration --emitDeclarationOnly --strict`
//! under each of `strictNullChecks` and `noImplicitAny` on and off and
//! read off each function's emitted return type; the four settings agree
//! but for the `| null` the `return null` adds under `strictNullChecks`.

use super::flow_return_class_tests::{assert_probes, flow_key, with_dispatch};
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue,
};
use verter_type_expr::facts::FunctionPartIdentity;

const FIXTURE: &str = "\
class D0 { p = 1 as const; private q = 2; protected r = 3 }
class Q0 { p = 1 as const; private q = 2; protected r = 3 }
class PR { protected x = 1 }
class SubPR2 extends PR { protected x = 2 }
interface A { kind: 'a'; a: 1 }
interface B { kind: 'b'; b: 2 }
interface C { c: 3 }
declare const d0: D0;
declare const pr: PR;
declare const a: A;
declare const c: C;
declare const ab: A | B;
declare const s: string;
declare const k: 'a';
declare const arr: number[];
declare const fn: () => void;
declare const ns: { inner: { d: D0 } };
declare const dn: D0 | undefined;
export function narrowTwin(x: D0 | Q0) { if (x === d0) { return x; } return null; }
export function narrowTwinElse(x: D0 | Q0) { if (x !== d0) { return x; } return null; }
export function narrowSub(x: SubPR2 | Q0) { if (x === pr) { return x; } return null; }
export function narrowObj(x: A | B) { if (x === a) { return x; } return null; }
export function narrowObjElse(x: A | B) { if (x !== a) { return x; } return null; }
export function narrowUnrelated(x: A | C) { if (x === c) { return x; } return null; }
export function narrowUnion(x: A | B | C) { if (x === ab) { return x; } return null; }
export function narrowLoose(x: D0 | Q0) { if (x == d0) { return x; } return null; }
export function narrowRev(x: D0 | Q0) { if (d0 === x) { return x; } return null; }
export function narrowPrim(x: D0 | string) { if (x === d0) { return x; } return null; }
export function eqUnknown(x: unknown) { if (x === d0) { return x; } return null; }
export function eqUnknownLoose(x: unknown) { if (x == d0) { return x; } return null; }
export function eqEmpty(x: {} | number) { if (x === d0) { return x; } return null; }
export function eqUnknownPrim(x: unknown) { if (x === s) { return x; } return null; }
export function eqPrim(x: 'a' | number) { if (x === s) { return x; } return null; }
export function eqPrimElse(x: 'a' | number) { if (x !== s) { return x; } return null; }
export function eqArr(x: string[] | number[]) { if (x === arr) { return x; } return null; }
export function eqFn(x: (() => void) | number) { if (x === fn) { return x; } return null; }
export function eqMember(x: D0 | A) { if (x === ns.inner.d) { return x; } return null; }
export function eqNullableValue(x: D0 | A) { if (x === dn) { return x; } return null; }
export function eqAny(x: any) { if (x === d0) { return x; } return null; }
export function eqFnObj(x: (() => void) | A) { if (x === fn) { return x; } return null; }
export function eqLit(x: 'a' | 'b') { if (x === k) { return x; } return null; }
";

/// The positive edge keeps the arms comparable to the value's type: `Q0`,
/// redeclaring `D0`'s private member, is not comparable to `D0`, nor
/// `SubPR2`'s sibling `Q0` to `PR`; `SubPR2`, whose protected member
/// overrides `PR`'s, is.
#[test]
fn equality_with_a_class_instance_keeps_the_comparable_arms() {
    assert_probes(
        FIXTURE,
        &[
            ("narrowTwin", "D0 | null"),
            ("narrowRev", "D0 | null"),
            ("narrowLoose", "D0 | null"),
            ("narrowSub", "SubPR2 | null"),
            ("narrowPrim", "D0 | null"),
            ("eqMember", "D0 | null"),
            ("eqNullableValue", "D0 | null"),
        ],
    );
}

/// Object, array, function and union values filter the same way.
#[test]
fn equality_with_an_object_value_keeps_the_comparable_arms() {
    assert_probes(
        FIXTURE,
        &[
            ("narrowObj", "A | null"),
            ("narrowUnrelated", "C | null"),
            ("narrowUnion", "A | B | null"),
            ("eqArr", "number[] | null"),
            ("eqFn", "(() => void) | null"),
            ("eqPrim", "\"a\" | null"),
        ],
    );
}

/// The negated edge narrows only against a unit type, which none of these
/// values is.
#[test]
fn inequality_with_a_non_unit_value_keeps_the_declared_type() {
    assert_probes(
        FIXTURE,
        &[
            ("narrowTwinElse", "D0 | Q0 | null"),
            ("narrowObjElse", "A | B | null"),
            ("eqPrimElse", "number | \"a\" | null"),
        ],
    );
}

/// `unknown` and a type with an empty object arm read, on the strict
/// positive edge, `object` for an object value and the value's own type
/// for a primitive one; the loose operator filters instead, and `any`
/// stays `any`.
#[test]
fn equality_substitutes_the_value_for_unknown_and_the_empty_object() {
    assert_probes(
        FIXTURE,
        &[
            ("eqUnknown", "object | null"),
            ("eqEmpty", "object | null"),
            ("eqUnknownPrim", "string | null"),
            ("eqUnknownLoose", "unknown"),
            ("eqAny", "any"),
        ],
    );
}

/// A literal value keeps the typed guard gap: the checker substitutes the
/// literal for a primitive arm (`eqLit` is `"a" | null` on 7.0.2). So does
/// an arm the relation authority neither separates from the value nor
/// relates to it: `eqFnObj` is `(() => void) | null` on 7.0.2, and the
/// authority does not read `A` against a function type.
#[test]
fn an_equality_the_relation_cannot_decide_keeps_the_guard_gap() {
    for name in ["eqLit", "eqFnObj"] {
        with_dispatch(FIXTURE, |dispatch| {
            let key = flow_key(dispatch, name, FunctionPartIdentity::DeclarationBody);
            let crate::semantic_query::QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::FlowReturn(result),
                ..
            }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
            else {
                panic!("{name} must produce a value");
            };
            assert_eq!(
                result.degradation(),
                Some(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing)),
                "{name} keeps the guard gap"
            );
        });
    }
}

const COMPARABLE: &str = "\
interface KA { kind: 'a'; a: 1 }
interface KC { c: 3 }
interface KAC { kind: 'a'; a: 1; c: 3 }
interface OA { a?: 1 }
interface OB { b?: 2 }
";

/// The comparable relation the narrowing filters by refuses a pair that
/// each requires a property the other lacks, and relates two arrays by
/// their elements.
///
/// Measured on TypeScript 7.0.2 over `declare const` values (all four
/// settings): `ka === kc`, `sa === na` (`string[]`, `number[]`) and `rsa
/// === na` (`readonly string[]`) are TS2367 ("the types … have no
/// overlap"); `ka === kac`, `oa === ob` and `sa === rsa` are accepted.
#[test]
fn comparability_reads_required_properties_and_array_elements() {
    let rows = [
        ("KA", "KC", "disjoint"),
        ("KA", "KAC", "overlaps"),
        ("OA", "OB", "overlaps"),
        ("string[]", "number[]", "disjoint"),
        ("readonly string[]", "number[]", "disjoint"),
        ("string[]", "readonly string[]", "overlaps"),
    ];
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|(a, b, expected)| {
            let measured = super::checker_probe_lane_tests::with_probe(
                COMPARABLE,
                &format!("[{a}, {b}]"),
                |dispatch, node| {
                    let Some(crate::semantic_query::SemanticNodeData::Tuple { elements, .. }) =
                        dispatch.graph().node_data(node).as_deref().cloned()
                    else {
                        panic!("`[{a}, {b}]` settles to a tuple");
                    };
                    match dispatch.nodes_comparable(elements[0].value, elements[1].value) {
                        super::relation::ComparabilityVerdict::Overlaps => "overlaps",
                        super::relation::ComparabilityVerdict::Disjoint(_) => "disjoint",
                        super::relation::ComparabilityVerdict::Undecided => "undecided",
                    }
                },
            );
            (measured != *expected)
                .then(|| format!("`{a}` / `{b}`: expected {expected}, measured {measured}"))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
