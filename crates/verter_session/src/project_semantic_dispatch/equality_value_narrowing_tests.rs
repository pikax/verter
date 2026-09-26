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

/// A literal value narrows by comparability like any value: `eqLit` over
/// `x: 'a' | 'b'` and `k: 'a'` is `"a" | null` on 7.0.2. An arm the
/// relation authority neither separates from a function value nor relates
/// to it keeps the typed gap (see
/// `a_function_value_is_not_comparable_to_an_object_arm`).
#[test]
fn an_equality_the_relation_cannot_decide_keeps_the_guard_gap() {
    assert_probes(FIXTURE, &[("eqLit", "\"a\" | null")]);
    with_dispatch(FIXTURE, |dispatch| {
        let key = flow_key(dispatch, "eqFnObj", FunctionPartIdentity::DeclarationBody);
        let crate::semantic_query::QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
        else {
            panic!("eqFnObj must produce a value");
        };
        assert_eq!(
            result.degradation(),
            Some(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing)),
            "eqFnObj keeps the guard gap"
        );
    });
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

const OPERANDS: &str = "\
class D0 { p = 1 as const; private q = 2; protected r = 3 }
class Q0 { p = 1 as const; private q = 2; protected r = 3 }
interface A { kind: 'a'; a: 1 }
interface B { kind: 'b'; b: 2 }
interface C { kind: string; c: 3 }
interface HA { k: A; tag: 'x' }
interface HB { k: B; tag: 'y' }
interface HS { k: string; n: 1 }
interface HN { k: number; n: 2 }
declare const k: 'a';
declare const kab: 'a' | 'b';
declare const one: 1;
declare const t: true;
declare const s: string;
declare const nul: null;
declare const und: undefined;
declare const a: A;
declare const fn: () => void;
declare const arr: number[];
declare function get(): D0;
declare function getK(): 'a';
declare function getS(): string;
const ck = 'a';
export function litDecl(x: 'a' | 'b') { if (x === k) { return x; } return null; }
export function litDeclElse(x: 'a' | 'b') { if (x !== k) { return x; } return null; }
export function litStr(x: string) { if (x === k) { return x; } return null; }
export function litStrElse(x: string) { if (x !== k) { return x; } return null; }
export function litStrNum(x: string | number) { if (x === k) { return x; } return null; }
export function litUnion(x: string) { if (x === kab) { return x; } return null; }
export function litUnionElse(x: 'a' | 'b' | 'c') { if (x !== kab) { return x; } return null; }
export function litNum(x: number | string) { if (x === one) { return x; } return null; }
export function litNumElse(x: 1 | 2) { if (x !== one) { return x; } return null; }
export function litBool(x: boolean) { if (x === t) { return x; } return null; }
export function litBoolElse(x: boolean) { if (x !== t) { return x; } return null; }
export function litNull(x: D0 | null) { if (x === nul) { return x; } return null; }
export function litNullElse(x: D0 | null) { if (x !== nul) { return x; } return null; }
export function litUndef(x: D0 | undefined) { if (x === und) { return x; } return null; }
export function litUndefLoose(x: D0 | undefined | null) { if (x == und) { return x; } return null; }
export function litUnknown(x: unknown) { if (x === k) { return x; } return null; }
export function litLoose(x: string | number) { if (x == k) { return x; } return null; }
export function litLooseNum(x: string | number) { if (x == one) { return x; } return null; }
export function litStrLoose(x: string) { if (x == s) { return x; } return null; }
export function memLeaf(x: HA | HB) { if (x.k === a) { return x.k; } return null; }
export function memParent(x: HA | HB) { if (x.k === a) { return x; } return null; }
export function memParentElse(x: HA | HB) { if (x.k !== a) { return x; } return null; }
export function memDisc(x: A | B) { if (x.kind === k) { return x; } return null; }
export function memDiscElse(x: A | B) { if (x.kind !== k) { return x; } return null; }
export function memDiscLeaf(x: A | B) { if (x.kind === k) { return x.kind; } return null; }
export function memNonDisc(x: HS | HN) { if (x.k === s) { return x; } return null; }
export function memNonDiscLeaf(x: HS | HN) { if (x.k === s) { return x.k; } return null; }
export function memLitNonDisc(x: HS | HN) { if (x.k === 'a') { return x; } return null; }
export function memLitWide(x: A | B | C) { if (x.kind === 'a') { return x; } return null; }
export function memLitWideElse(x: A | B | C) { if (x.kind !== 'a') { return x; } return null; }
export function both(x: A | B, y: B | D0) { if (x === y) { return { x, y }; } return null; }
export function bothElse(x: A | B, y: B | D0) { if (x !== y) { return { x, y }; } return null; }
export function bothLit(x: 'a' | 'b', y: 'b' | 'c') { if (x === y) { return { x, y }; } return null; }
export function bothLitElse(x: 'a' | 'b', y: 'b') { if (x !== y) { return { x, y }; } return null; }
export function bothMember(x: A | B, o: { v: B | D0 }) { if (x === o.v) { return { x, v: o.v }; } return null; }
export function call(x: D0 | Q0) { if (x === get()) { return x; } return null; }
export function callRev(x: D0 | Q0) { if (get() === x) { return x; } return null; }
export function callLit(x: 'a' | 'b') { if (x === getK()) { return x; } return null; }
export function callStr(x: 'a' | number) { if (x === getS()) { return x; } return null; }
export function fnObj(x: (() => void) | A) { if (x === fn) { return x; } return null; }
export function arrObj(x: number[] | A) { if (x === arr) { return x; } return null; }
export function objArr(x: number[] | A) { if (x === a) { return x; } return null; }
export function objFn(x: (() => void) | A) { if (x === a) { return x; } return null; }
export function constRead() { return ck; }
";

/// A literal-typed value narrows by comparability like any value: the
/// positive edge keeps the comparable arms and replaces a `string` /
/// `number` arm by the value's literals of its kind, the negated edge
/// removes the unit arm equal to a unit value, `null` / `undefined`
/// select or remove the nullish arms (`==` both), `unknown` reads the
/// value itself, and `==` with a non-literal primitive keeps the arms it
/// coerces.
///
/// Measured on TypeScript 7.0.2 with `--strict` (the prints below; with
/// `strictNullChecks` off the `| null` from `return null` and the nullish
/// arms are erased, `litNull`, `litUndef` and `litUndefLoose` read `D0`,
/// and the other answers agree; `noImplicitAny` changes nothing).
#[test]
fn a_literal_typed_value_narrows_by_comparability() {
    assert_probes(
        OPERANDS,
        &[
            ("litDecl", "\"a\" | null"),
            ("litDeclElse", "\"b\" | null"),
            ("litStr", "\"a\" | null"),
            ("litStrElse", "string | null"),
            ("litStrNum", "\"a\" | null"),
            ("litUnion", "\"a\" | \"b\" | null"),
            ("litUnionElse", "\"a\" | \"b\" | \"c\" | null"),
            ("litNum", "1 | null"),
            ("litNumElse", "2 | null"),
            ("litBool", "true | null"),
            ("litBoolElse", "false | null"),
            ("litNull", "null"),
            ("litNullElse", "D0 | null"),
            ("litUndef", "null | undefined"),
            ("litUndefLoose", "null | undefined"),
            ("litUnknown", "\"a\" | null"),
            ("litLoose", "\"a\" | null"),
            ("litLooseNum", "1 | null"),
            ("litStrLoose", "string | null"),
        ],
    );
}

/// A member reference narrows by the value, and its parent only when the
/// member is a discriminant (the arms' member types differ and one is a
/// literal type): the parent keeps the arms whose member type is
/// comparable to the narrowed member type.
///
/// Measured on TypeScript 7.0.2 with `--strict` (all four settings agree
/// but for the `| null`): `x.k === a` over `HA | HB` narrows `x.k` to `A`
/// and keeps `x` (`A` and `B` are not literal types), `x.kind === k`
/// selects `A` (`B` on `!==`) and narrows `x.kind` to `"a"`, `x.k === s`
/// over `HS | HN` keeps `x` and narrows `x.k` to `string`; with a literal
/// operand, `x.k === 'a'` over `HS | HN` keeps both arms, `x.kind ===
/// 'a'` over `A | B | C` (`C`'s `kind` is `string`) is `A | C` and
/// `x.kind !== 'a'` keeps all three.
#[test]
fn a_member_reference_narrows_and_its_parent_by_the_discriminant_rule() {
    assert_probes(
        OPERANDS,
        &[
            ("memLeaf", "A | null"),
            ("memParent", "HA | HB | null"),
            ("memParentElse", "HA | HB | null"),
            ("memDisc", "A | null"),
            ("memDiscElse", "B | null"),
            ("memDiscLeaf", "\"a\" | null"),
            ("memNonDisc", "HN | HS | null"),
            ("memNonDiscLeaf", "string | null"),
            ("memLitNonDisc", "HN | HS | null"),
            ("memLitWide", "A | C | null"),
            ("memLitWideElse", "A | B | C | null"),
        ],
    );
}

/// Two references both narrow, each by the other's type at the test.
///
/// Measured on TypeScript 7.0.2 with `--strict` (all four settings agree
/// but for the `| null`): `{ x, y }` inside `x === y` over `x: A | B`, `y:
/// B | D0` is `{ x: B; y: B; }` and keeps both types on `!==`; over `'a' |
/// 'b'` and `'b' | 'c'` it is `{ x: "b"; y: "b"; }`; inside `x !== y` over
/// `'a' | 'b'` and `'b'` it is `{ x: "a"; y: "b"; }`; `x === o.v` narrows the
/// member reference too.
#[test]
fn two_references_narrow_each_other() {
    assert_probes(
        OPERANDS,
        &[
            ("both", "{ x: B; y: B; } | null"),
            ("bothElse", "{ x: A | B; y: B | D0; } | null"),
            ("bothLit", "{ x: \"b\"; y: \"b\"; } | null"),
            ("bothLitElse", "{ x: \"a\"; y: \"b\"; } | null"),
            ("bothMember", "{ x: B; v: B; } | null"),
        ],
    );
}

/// A value read through a call narrows by the call's return type, in
/// either operand order.
///
/// Measured on TypeScript 7.0.2 with `--strict` (all four settings agree
/// but for the `| null`): over `declare function get(): D0`, `x ===
/// get()` and `get() === x` over `D0 | Q0` are `D0`; over `getK(): 'a'`
/// and `getS(): string`, `x === getK()` is `"a"` and `x === getS()` over
/// `'a' | number` is `"a"`.
#[test]
fn a_call_value_narrows_by_its_return_type() {
    assert_probes(
        OPERANDS,
        &[
            ("call", "D0 | null"),
            ("callRev", "D0 | null"),
            ("callLit", "\"a\" | null"),
            ("callStr", "\"a\" | null"),
        ],
    );
}

/// A function value and an object arm, and an array and an object, are
/// comparable in neither direction: an object lacks the call signature
/// and the function's apparent members lack the object's properties, and
/// the array's apparent `Array` members and the object's properties are
/// each missing from the other. Measured on TypeScript 7.0.2 with
/// `--strict` (the four settings agree but for the `| null`): `fnObj` is
/// `(() => void) | null`, `arrObj` `number[] | null`, `objArr` and `objFn`
/// `A | null`.
#[test]
#[ignore = "the comparable relation separates a signature or array type from an object type"]
fn a_function_value_is_not_comparable_to_an_object_arm() {
    assert_probes(
        OPERANDS,
        &[
            ("fnObj", "(() => void) | null"),
            ("arrObj", "number[] | null"),
            ("objArr", "A | null"),
            ("objFn", "A | null"),
        ],
    );
}

/// A `const` initialized with a literal has the checker's fresh literal
/// type, which widens as a function's sole return. Measured on TypeScript
/// 7.0.2 (all four settings): `const ck = 'a'; function constRead() {
/// return ck; }` returns `string`.
#[test]
fn a_const_literal_binding_widens_as_the_sole_return() {
    assert_probes(OPERANDS, &[("constRead", "string")]);
}
