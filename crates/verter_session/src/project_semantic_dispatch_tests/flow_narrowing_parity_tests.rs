//! Narrowing forms the flow lane models as the checker does, each read
//! back as a flow-return answer and, where the form is a returned test, as
//! the type predicate the checker infers from it.
//!
//! Checker-parity answers are measured on the pinned TypeScript 7.0.2: the
//! strict column is the function's `.d.ts` line (`tsc --declaration
//! --emitDeclarationOnly --strict`), the loose column the same under
//! `--strictNullChecks false`, read from a TS2322 message quoting the type
//! where the loose `.d.ts` reprints an authored annotation instead. A
//! branch that does not return `throw`s, so each answer is the narrowed
//! read alone, never a join.
//! Symbol-abbreviation controls instead pin the documented stable-spelling
//! policy where the native checker's messages depend on process history.

use super::signature_predicate_inference_tests::{
    assert_predicate, assert_prints, host_with, LOOSE_ROOT, STRICT_ROOT,
};

/// Run `(function, strict print, loose print)` rows over `source`.
fn check_rows(source: &str, rows: &[(&str, &str, &str)]) {
    let host = host_with(source);
    for (function, strict, loose) in rows {
        assert_prints(&host, STRICT_ROOT, function, strict);
        assert_prints(&host, LOOSE_ROOT, function, loose);
    }
}

/// Run `(function, strict print, loose print)` rows over `source` whose
/// answer is a predicate signature: the whole print, and the predicate
/// (or its absence) beside the `boolean` return.
fn check_predicate_rows(source: &str, rows: &[(&str, &str, &str)]) {
    let host = host_with(source);
    for (function, strict, loose) in rows {
        for (root, printed) in [(STRICT_ROOT, strict), (LOOSE_ROOT, loose)] {
            assert_prints(&host, root, function, printed);
            assert_predicate(&host, root, function, printed);
        }
    }
}

const TOP_EQUALITY: &str = r#"
export function a1(x: unknown) { if (x !== null) return x; throw 0; }
export function a2(x: unknown) { if (x !== undefined) return x; throw 0; }
export function a3(x: unknown) { if (x != null) return x; throw 0; }
export function a4(x: unknown) { if (x != undefined) return x; throw 0; }
export function a5(x: unknown) { if (x === null) return x; throw 0; }
export function a6(x: unknown) { if (x === undefined) return x; throw 0; }
export function a7(x: unknown) { if (x == null) return x; throw 0; }
export function a9(x: unknown) { if (x !== null && x !== undefined) return x; throw 0; }
export function b1(x: any) { if (x !== null) return x; throw 0; }
export function b3(x: any) { if (x != null) return x; throw 0; }
export function b4(x: any) { if (x == null) return x; throw 0; }
export function t1(x: unknown) { if (x) return x; throw 0; }
export function t2(x: unknown) { if (!x) return x; throw 0; }
export function t4(x: any) { if (x) return x; throw 0; }
"#;

/// A nullish or truthiness test over a TOP subject. `unknown` splits as
/// the checker's `{} | null | undefined` under `strictNullChecks` and
/// stays `unknown` without it; `any` never narrows.
#[test]
fn a_nullish_or_truthy_test_over_unknown_or_any_narrows_as_the_checker_does() {
    check_rows(
        TOP_EQUALITY,
        &[
            ("a1", "{} | undefined", "unknown"),
            ("a2", "{} | null", "unknown"),
            ("a3", "{}", "unknown"),
            ("a4", "{}", "unknown"),
            ("a5", "null", "unknown"),
            ("a6", "undefined", "unknown"),
            ("a7", "null | undefined", "unknown"),
            ("a9", "{}", "unknown"),
            ("b1", "any", "any"),
            ("b3", "any", "any"),
            ("b4", "any", "any"),
            ("t1", "{}", "unknown"),
            ("t2", "unknown", "unknown"),
            ("t4", "any", "any"),
        ],
    );
}

const TOP_PREDICATES: &str = r#"
export function p1(x: unknown) { return x !== null; }
export function p2(x: unknown) { return x !== undefined; }
export function p3(x: unknown) { return x != null; }
export function p4(x: unknown) { return x == null; }
export function p5(x: unknown) { return x === null; }
export function p6(x: unknown) { return x === undefined; }
export function p7(x: any) { return x !== null; }
export function p8(x: any) { return x === null; }
export function p9(x: any) { return x != null; }
export function sig_p1() { return p1; }
export function sig_p2() { return p2; }
export function sig_p3() { return p3; }
export function sig_p4() { return p4; }
export function sig_p5() { return p5; }
export function sig_p6() { return p6; }
export function sig_p7() { return p7; }
export function sig_p8() { return p8; }
export function sig_p9() { return p9; }
"#;

/// The same tests returned: the inferred predicate reads the split.
/// Without `strictNullChecks` none narrows, so none infers a predicate;
/// over `any` none narrows either.
#[test]
fn a_returned_nullish_test_over_unknown_infers_the_checker_predicate() {
    check_predicate_rows(
        TOP_PREDICATES,
        &[
            (
                "sig_p1",
                "(x: unknown) => x is {} | undefined",
                "(x: unknown) => boolean",
            ),
            (
                "sig_p2",
                "(x: unknown) => x is {} | null",
                "(x: unknown) => boolean",
            ),
            (
                "sig_p3",
                "(x: unknown) => x is {}",
                "(x: unknown) => boolean",
            ),
            (
                "sig_p4",
                "(x: unknown) => x is null | undefined",
                "(x: unknown) => boolean",
            ),
            (
                "sig_p5",
                "(x: unknown) => x is null",
                "(x: unknown) => boolean",
            ),
            (
                "sig_p6",
                "(x: unknown) => x is undefined",
                "(x: unknown) => boolean",
            ),
            ("sig_p7", "(x: any) => boolean", "(x: any) => boolean"),
            ("sig_p8", "(x: any) => boolean", "(x: any) => boolean"),
            ("sig_p9", "(x: any) => boolean", "(x: any) => boolean"),
        ],
    );
}

const LOOSE_EQUALITY: &str = r#"
export function l1(x: string | null | undefined) { if (x != null) return x; throw 0; }
export function l2(x: string | null | undefined) { if (x == null) return x; throw 0; }
export function l3(x: string | null) { if (x == undefined) return x; throw 0; }
export function l5(x: string | null | undefined) { if (null != x) return x; throw 0; }
export function l6(x: string | undefined) { if (x === null) return x; throw 0; }
export function looseNull(x: string | null | undefined) { return x == null; }
export function looseUndef(x: string | undefined) { return x != undefined; }
export function sig_looseNull() { return looseNull; }
export function sig_looseUndef() { return looseUndef; }
"#;

/// Loose equality against `null` or `undefined` selects both nullish arms
/// (`x == null` is `null | undefined`, `x != null` removes both), in either
/// operand order. Without `strictNullChecks` a nullish comparison narrows
/// nothing — the strict `x === null` over `string | undefined` included,
/// which is `never` with the option on.
#[test]
fn loose_equality_against_null_selects_both_nullish_arms() {
    check_rows(
        LOOSE_EQUALITY,
        &[
            ("l1", "string", "string"),
            ("l2", "null | undefined", "string"),
            ("l3", "null", "string"),
            ("l5", "string", "string"),
            ("l6", "never", "string"),
        ],
    );
    check_predicate_rows(
        LOOSE_EQUALITY,
        &[
            (
                "sig_looseNull",
                "(x: string | null | undefined) => x is null | undefined",
                "(x: string) => boolean",
            ),
            (
                "sig_looseUndef",
                "(x: string | undefined) => x is string",
                "(x: string) => boolean",
            ),
        ],
    );
}

const TYPEOF_NAMED_ARMS: &str = r#"
export interface Foo { kind: 'foo'; n: number }
export interface Bar { kind: 'bar'; s: string }
export interface Callable { (): void; tag: string }
export class Cls { c = 1 }
type Obj = { o: 1 };
export function y1(x: Foo | Bar | number) { if (typeof x === "object") return x; throw 0; }
export function y2(x: Foo | Bar | number) { if (typeof x !== "object") return x; throw 0; }
export function y3(x: Cls | string) { if (typeof x === "object") return x; throw 0; }
export function y4(x: Callable | Foo) { if (typeof x === "function") return x; throw 0; }
export function y5(x: Callable | Foo) { if (typeof x === "object") return x; throw 0; }
export function y6(x: Obj | boolean) { if (typeof x === "object") return x; throw 0; }
export function hasKind(x: Foo | Bar | number) { return typeof x === "object" && "s" in x; }
export function orIn(x: Foo | Bar | number) { return typeof x === "number" || "n" in x; }
export function isObj(x: Foo | number) { return typeof x === "object"; }
export function sig_hasKind() { return hasKind; }
export function sig_orIn() { return orIn; }
export function sig_isObj() { return isObj; }
"#;

/// A `typeof` test classifies a NAMED arm — an interface, a class
/// instance, an alias — by the structure it denotes, and an object type
/// with a call signature is a `"function"`, not an `"object"`.
#[test]
fn a_typeof_test_classifies_named_and_callable_object_arms() {
    check_rows(
        TYPEOF_NAMED_ARMS,
        &[
            ("y1", "Bar | Foo", "Bar | Foo"),
            ("y2", "number", "number"),
            ("y3", "Cls", "Cls"),
            ("y4", "Callable", "Callable"),
            ("y5", "Foo", "Foo"),
            ("y6", "Obj", "Obj"),
        ],
    );
    check_predicate_rows(
        TYPEOF_NAMED_ARMS,
        &[
            (
                "sig_hasKind",
                "(x: Foo | Bar | number) => x is Bar",
                "(x: Foo | Bar | number) => x is Bar",
            ),
            (
                "sig_orIn",
                "(x: Foo | Bar | number) => x is number | Foo",
                "(x: Foo | Bar | number) => x is number | Foo",
            ),
            (
                "sig_isObj",
                "(x: Foo | number) => x is Foo",
                "(x: Foo | number) => x is Foo",
            ),
        ],
    );
}

const BOOLEAN_LITERAL_UNIONS: &str = r#"
export function tern(x: unknown) { return typeof x === "string" ? true : false; }
export function tern2(c: boolean) { return c ? true : false; }
export function tern3(c: boolean) { return c ? true : "x"; }
export function tern4(c: number) { return c > 1 ? true : c > 0 ? false : "x"; }
export function joined(c: boolean) { if (c) return true; return false; }
export function sig_tern() { return tern; }
"#;

/// `true | false` IS `boolean`: a join or a ternary over both literals is
/// `boolean` (beside a third arm too), one literal stays itself, and the
/// ternary is no type predicate. Measured on 7.0.2: `boolean`, `boolean`,
/// `"x" | true`, `"x" | boolean`, `boolean`, `(x: unknown) => boolean`.
#[test]
fn true_and_false_join_to_boolean() {
    check_rows(
        BOOLEAN_LITERAL_UNIONS,
        &[
            ("tern", "boolean", "boolean"),
            ("tern2", "boolean", "boolean"),
            ("tern3", "\"x\" | true", "\"x\" | true"),
            ("tern4", "\"x\" | boolean", "\"x\" | boolean"),
            ("joined", "boolean", "boolean"),
            (
                "sig_tern",
                "(x: unknown) => boolean",
                "(x: unknown) => boolean",
            ),
        ],
    );
}

const LOGICAL_NOT: &str = r#"
export function n1(o: { a: 1 }) { return !o; }
export function n3(s: string) { return !s; }
export function n7(u: unknown) { return !u; }
export function n10(o: { a: 1 }) { const v = !o; return v; }
export function n11(o: { a: 1 }) { return { v: !o }; }
export function n12(o: { a: 1 }) { const v = !!o; return v; }
export function k1(o: { a: 1 }) { const v: false = !o; return v; }
export function k2(e: "" | 0) { const v: true = !e; return v; }
export function k3(o: { a: 1 }) { if (!o) return 1; return 2; }
export function k4(o: { a: 1 }) { return { v: !o } as const; }
export function k5(e: "" | 0) { return { v: !e } as const; }
export function exact(o: { a: 1 }, e: "" | 0, a: "a" | 1, s: string) { if (s) return !o; if (a) return !e; return !a; }
export function truthy(x: string | undefined) { return !!x; }
export function notTruthy(x: string | undefined) { return !x; }
export function truthyObj(x: { a: 1 } | undefined) { return !!x; }
export function notTruthyObj(x: { a: 1 } | undefined) { return !x; }
export function n13(o: { a: 1 } | null) { return !o; }
export function sig_truthy() { return truthy; }
export function sig_notTruthy() { return notTruthy; }
export function sig_truthyObj() { return truthyObj; }
export function sig_notTruthyObj() { return notTruthyObj; }
export function sig_n13() { return n13; }
"#;

/// `!x` over any operand: `false` when the operand can only be truthy,
/// `true` when it can only be falsy, `boolean` otherwise — a fresh literal
/// that a lone return, a `const` read back and an object member widen.
/// Without `strictNullChecks` an operand that can be truthy can also be
/// nullish. Measured on 7.0.2 (strict / loose): `!{ a: 1 }` is `false` /
/// `boolean`, `!("" | 0)` is `true` / `true`, `!("a" | 1)` is `false` /
/// `boolean` (read through annotated `const`s and a three-arm join that
/// keeps the literals, and a `const` assertion over it),
/// and every other row is the checker's `.d.ts`.
#[test]
fn a_logical_not_is_typed_by_the_operand_truthiness() {
    check_rows(
        LOGICAL_NOT,
        &[
            ("n1", "boolean", "boolean"),
            ("n3", "boolean", "boolean"),
            ("n7", "boolean", "boolean"),
            ("n10", "boolean", "boolean"),
            ("n11", "{ v: boolean; }", "{ v: boolean; }"),
            ("n12", "boolean", "boolean"),
            ("k1", "false", "false"),
            ("k2", "true", "true"),
            ("k3", "1 | 2", "1 | 2"),
            ("k4", "{ readonly v: false; }", "{ readonly v: boolean; }"),
            ("k5", "{ readonly v: true; }", "{ readonly v: true; }"),
            ("exact", "boolean", "boolean"),
        ],
    );
    check_predicate_rows(
        LOGICAL_NOT,
        &[
            (
                "sig_truthy",
                "(x: string | undefined) => boolean",
                "(x: string) => boolean",
            ),
            (
                "sig_notTruthy",
                "(x: string | undefined) => boolean",
                "(x: string) => boolean",
            ),
            (
                "sig_truthyObj",
                "(x: { a: 1; } | undefined) => x is { a: 1; }",
                "(x: { a: 1; }) => boolean",
            ),
            (
                "sig_notTruthyObj",
                "(x: { a: 1; } | undefined) => x is undefined",
                "(x: { a: 1; }) => boolean",
            ),
            (
                "sig_n13",
                "(o: { a: 1; } | null) => o is null",
                "(o: { a: 1; }) => boolean",
            ),
        ],
    );
}

const ALIASED_CONDITIONS: &str = r#"
type A = { kind: "a"; a: string };
type B = { kind: "b"; b: number };
export function c1(x: string | number) { const isStr = typeof x === "string"; if (isStr) return x; throw 0; }
export function c2(x: string | number) { const isStr = typeof x === "string"; if (!isStr) return x; throw 0; }
export function c3(x: string | number) { let isStr = typeof x === "string"; if (isStr) return x; throw 0; }
export function c4(x: string | number) { const isStr = typeof x === "string"; x = 1; if (isStr) return x; throw 0; }
export function c5(x: string | number) { const isStr = typeof x === "string"; if (isStr) return isStr; throw 0; }
export function c6(x: string | number) { const isStr: boolean = typeof x === "string"; if (isStr) return x; throw 0; }
export function c7(x: string | number | null) { const ok = x !== null && typeof x === "string"; if (ok) return x; throw 0; }
export function c8(x: string | number) { const a = typeof x === "string"; const b = a; if (b) return x; throw 0; }
export function d4(x: string | number) { const a = typeof x === "string"; const b = a; const c = b; const d = c; if (d) return x; throw 0; }
export function d5(x: string | number) { const a = typeof x === "string"; const b = a; const c = b; const d = c; const e = d; if (e) return x; throw 0; }
export function d6(x: string | number) { const a = typeof x === "string"; const b = a; const c = b; const d = c; const e = d; const f = e; if (f) return x; throw 0; }
export function e1(x: string | number) { let y = x; const isStr = typeof y === "string"; if (isStr) return y; throw 0; }
export function e2(x: string | number) { let y = x; const isStr = typeof y === "string"; y = 1; if (isStr) return y; throw 0; }
export function e3(x: string | number) { const isStr = typeof x === "string"; const g = () => { x = 2; }; if (isStr) return x; throw 0; }
export function e4(x: string | number) { const isStr = typeof x === "string" ? true : false; if (isStr) return x; throw 0; }
export function e6(x: string | number) { const isStr = typeof x === "string"; return isStr ? x : 0; }
export function k1(u: A | B) { const k = u.kind; if (k === "a") return u; throw 0; }
export function k2(u: A | B) { const { kind } = u; if (kind === "a") return u; throw 0; }
export function k3(u: A | B) { let k = u.kind; if (k === "a") return u; throw 0; }
export function viaLocal(x: unknown) { const r = typeof x === "string"; return r; }
export function viaLocal2(x: string | number) { const r = typeof x === "string"; const s = r; return s; }
export function viaLet(x: unknown) { let r = typeof x === "string"; return r; }
export function viaNot(x: string | number) { const r = typeof x === "string"; return !r; }
export function sig_viaLocal() { return viaLocal; }
export function sig_viaLocal2() { return viaLocal2; }
export function sig_viaLet() { return viaLet; }
export function sig_viaNot() { return viaNot; }
"#;

/// A `const` alias of a condition narrows the constant references its
/// initializer names at every test of the alias (TypeScript 4.4's aliased
/// conditions): not through a `let` alias, an annotated alias, or a
/// ternary of literals; not a parameter or `let` something assigns (a
/// closure's write included); and through at most five levels of alias
/// chains (`d5` narrows, `d6` does not). The alias narrows itself too
/// (`c5` is `true`). A `const` alias of a member (`const k = u.kind`, `const
/// { kind } = u`) is a discriminant of its object. The same holds for a
/// returned alias's inferred predicate. Measured identically under both
/// `strictNullChecks` settings.
#[test]
fn an_aliased_condition_narrows_like_its_initializer() {
    let rows = [
        ("c1", "string"),
        ("c2", "number"),
        ("c3", "string | number"),
        ("c4", "number"),
        ("c5", "true"),
        ("c6", "string | number"),
        ("c7", "string"),
        ("c8", "string"),
        ("d4", "string"),
        ("d5", "string"),
        ("d6", "string | number"),
        ("e1", "string"),
        ("e2", "number"),
        ("e3", "string | number"),
        ("e4", "string | number"),
        ("e6", "string | 0"),
        ("k1", "A"),
        ("k2", "A"),
        ("k3", "A | B"),
    ];
    let rows: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(function, printed)| (*function, *printed, *printed))
        .collect();
    check_rows(ALIASED_CONDITIONS, &rows);
    check_predicate_rows(
        ALIASED_CONDITIONS,
        &[
            (
                "sig_viaLocal",
                "(x: unknown) => x is string",
                "(x: unknown) => x is string",
            ),
            (
                "sig_viaLocal2",
                "(x: string | number) => x is string",
                "(x: string | number) => x is string",
            ),
            (
                "sig_viaLet",
                "(x: unknown) => boolean",
                "(x: unknown) => boolean",
            ),
            (
                "sig_viaNot",
                "(x: string | number) => x is number",
                "(x: string | number) => x is number",
            ),
        ],
    );
}

const CALL_PREDICATES: &str = r#"
export interface Foo { kind: 'foo'; n: number }
export interface Bar { kind: 'bar'; s: string }
interface ArrayConstructorLike { isArray(arg: any): arg is any[]; }
declare const Arrays: ArrayConstructorLike;
interface NumberConstructorLike { isInteger(number: unknown): boolean; }
declare const Numbers: NumberConstructorLike;
declare function isFoo(x: unknown): x is Foo;
declare function isKA(x: unknown): x is "a";
declare function isT<T>(x: unknown): x is T;
declare function ov(x: string): x is 'a';
declare function ov(x: number): x is 1;
declare function plain(x: unknown): boolean;
declare function second(a: unknown, b: unknown): b is Bar;
declare const obj: { guard(x: unknown): x is Bar };
export class Box { v: Foo | Bar = null!; isFoo(): this is { v: Foo } { return true; } }
export function i1(x: string | string[]) { if (Arrays.isArray(x)) return x; throw 0; }
export function i2(x: string | string[]) { if (!Arrays.isArray(x)) return x; throw 0; }
export function i3(x: unknown) { if (Arrays.isArray(x)) return x; throw 0; }
export function i4(x: number | readonly number[]) { if (Arrays.isArray(x)) return x; throw 0; }
export function c1(x: Foo | Bar) { if (isFoo(x)) return x; throw 0; }
export function c2(x: Foo | Bar) { if (obj.guard(x)) return x; throw 0; }
export function c3(x: unknown) { if (isT<string>(x)) return x; throw 0; }
export function c4(x: string | number) { if (ov(x as string)) return x; throw 0; }
export function c5(x: "a" | "b") { if (ov(x)) return x; throw 0; }
export function c6(x: Foo | Bar) { if (plain(x)) return x; throw 0; }
export function c7(x: Foo | Bar, y: Foo | Bar) { if (second(x, y)) return y; throw 0; }
export function c8(x: Foo | Bar, y: Foo | Bar) { if (second(x, y)) return x; throw 0; }
export function c9(x: Foo | Bar) { if (!isFoo(x)) return x; throw 0; }
export function c10(x: Foo | Bar | null) { if (isFoo(x) || x === null) return x; throw 0; }
export function c11(b: Box) { if (b.isFoo()) return b.v; throw 0; }
export function c12(x: Foo | Bar) { if (Numbers.isInteger(x)) return x; throw 0; }
export function c13(x: string | null | undefined) { if (Arrays.isArray(x)) return x; throw 0; }
export function c14(o: { x: Foo | Bar }) { if (isFoo(o["x"])) return o["x"]; throw 0; }
export function c15<T>(x: T, g: (v: unknown) => v is T[]) { if (g(x)) return x; throw 0; }
export function c17(o: { k: "a"; v: string } | { k: "b"; v: number }) { if (isKA(o.k)) return o; throw 0; }
export function c18(o: { x: Foo | Bar } | undefined) { if (isFoo(o?.x)) return o; throw 0; }
export function r1(x: string | string[]) { return Arrays.isArray(x); }
export function r2(x: unknown) { return Arrays.isArray(x); }
export function r3(x: Foo | Bar) { return isFoo(x); }
export function r4(x: Foo | Bar) { return obj.guard(x); }
export function r5(x: unknown) { return Numbers.isInteger(x); }
export function r6(x: Foo | Bar) { return !isFoo(x); }
export function r7(x: Foo | Bar, y: Foo | Bar) { return second(x, y); }
export function sig_r1() { return r1; }
export function sig_r2() { return r2; }
export function sig_r3() { return r3; }
export function sig_r4() { return r4; }
export function sig_r5() { return r5; }
export function sig_r6() { return r6; }
export function sig_r7() { return r7; }
"#;

const CALL_ROWS: &[(&str, &str, &str)] = &[
    ("i1", "string[]", "string[]"),
    ("i2", "string", "string"),
    ("i3", "any[]", "any[]"),
    ("i4", "any[]", "any[]"),
    ("c1", "Foo", "Foo"),
    ("c2", "Bar", "Bar"),
    ("c3", "string", "string"),
    ("c4", "string | number", "string | number"),
    ("c5", "\"a\"", "\"a\""),
    ("c6", "Bar | Foo", "Bar | Foo"),
    ("c7", "Bar", "Bar"),
    ("c8", "Bar | Foo", "Bar | Foo"),
    ("c9", "Bar", "Bar"),
    ("c10", "Foo | null", "Bar | Foo"),
    ("c11", "Foo", "Foo"),
    ("c12", "Bar | Foo", "Bar | Foo"),
    ("c13", "string & any[]", "string & any[]"),
    ("c14", "Foo", "Foo"),
    (
        "c17",
        "{ k: \"a\"; v: string; } | { k: \"b\"; v: number; }",
        "{ k: \"a\"; v: string; } | { k: \"b\"; v: number; }",
    ),
];

const CALL_PREDICATE_ROWS: &[(&str, &str, &str)] = &[
    (
        "sig_r1",
        "(x: string | string[]) => x is string[]",
        "(x: string | string[]) => x is string[]",
    ),
    (
        "sig_r2",
        "(x: unknown) => x is any[]",
        "(x: unknown) => x is any[]",
    ),
    (
        "sig_r3",
        "(x: Foo | Bar) => x is Foo",
        "(x: Foo | Bar) => x is Foo",
    ),
    (
        "sig_r4",
        "(x: Foo | Bar) => x is Bar",
        "(x: Foo | Bar) => x is Bar",
    ),
    (
        "sig_r5",
        "(x: unknown) => boolean",
        "(x: unknown) => boolean",
    ),
    (
        "sig_r6",
        "(x: Foo | Bar) => x is Bar",
        "(x: Foo | Bar) => x is Bar",
    ),
    (
        "sig_r7",
        "(x: Foo | Bar, y: Foo | Bar) => y is Bar",
        "(x: Foo | Bar, y: Foo | Bar) => y is Bar",
    ),
];

/// A call whose callee's signature carries a type predicate narrows the
/// argument (or the receiver, for `this is`) the predicate names: a
/// lib-shaped guard (`isArray(arg: any): arg is any[]`, declared here as
/// the lib declares it), a declared function, a method of a declared
/// object, a generic instantiated by its type argument, or the overload
/// the call selects. The candidate replaces an arm it is a subtype of,
/// keeps an arm that is a subtype of it, and otherwise intersects arm by
/// arm (`readonly number[]` narrows to `any[]`; `string | null |
/// undefined` to `string & any[]`, the nullish arms collapsing beside an
/// object type). A callee without a predicate, and an argument that is
/// not a reference (`x as string`), narrow nothing. Returned, the call is
/// the function's inferred predicate; a `this is` predicate narrows the
/// receiver (`c11`: `b` is `Box & { v: Foo }`, whose `v` is `Foo`).
/// Measured on 7.0.2, identically against the lib's own `Array.isArray`;
/// only `c10` differs without `strictNullChecks`, where `x === null`
/// narrows nothing.
///
/// A predicate over a member narrows the member, not a union parent
/// through it (`c17`); `g(x)` over a type parameter narrows it through its
/// constraint (`c15` is `T & T[]`, pinned by
/// [`a_type_parameter_narrows_through_its_constraint`]). `isFoo(o["x"])`
/// narrows the element access its literal key names (`c14` is `Foo` in
/// all four `strictNullChecks` × `noImplicitAny` projects). One call
/// degrades: `isFoo(o?.x)` narrows the optional chain's root (`c18` is
/// `{ x: Foo | Bar; }`), a reference this half does not carry.
#[test]
fn a_call_to_a_predicate_signature_narrows_its_argument() {
    check_rows(CALL_PREDICATES, CALL_ROWS);
    check_predicate_rows(CALL_PREDICATES, CALL_PREDICATE_ROWS);
    let host = host_with(CALL_PREDICATES);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        super::signature_predicate_inference_tests::assert_degrades(&host, root, "c18");
    }
}

const CALL_PREDICATE_SCOPES: &str = r#"
export interface Foo { kind: 'foo'; n: number }
export interface Bar { kind: 'bar'; s: string }
export interface C { c: 1 }
type T = number;
const y = 1;
function isNum(x: unknown): x is T { return true }
function isY(x: unknown): x is typeof y { return true }
function isSame<T>(x: T): x is T { return true }
function pred(x: string): x is string;
function pred(x: string | number): x is number;
function pred(x: string | number): boolean { return typeof x === "number"; }
declare function isC(x: unknown): x is C;
export function g1<T extends number>(x: number | string, _t: T) { if (isNum(x)) return x; throw 0; }
export function g2(x: number | string) { type T = string; if (isNum(x)) return x; throw 0; }
export function g3(x: number | string) { class T { t = 1 }; if (isNum(x)) return x; throw 0; }
export function g4(x: number | string) { const y = "s"; if (isY(x)) return x; throw 0; }
export function g5(x: number | string, y: string) { if (isY(x)) return x; throw 0; }
export function g6(x: string | number) { if (isSame(x)) return x; throw 0; }
export function g8<T extends number>() { return (x: number | string) => { if (isNum(x)) return x; throw 0; }; }
export function g9(x: string | number) { if (pred(x)) return x; throw 0; }
export function g10(x: string | number) { if (!pred(x)) return x; throw 0; }
export function o1(x: Foo | Bar) { if (isC(x)) return x; throw 0; }
export function o2(x: Foo | Bar | null) { if (isC(x)) return x; throw 0; }
export function o3(x: Foo | Bar | null | undefined) { if (isC(x)) return x; throw 0; }
export function o4(x: Foo | null) { if (isC(x)) return x; throw 0; }
"#;

/// A predicate's target is the CALLEE's, instantiated by the call: it
/// resolves where the callee is declared, whatever the caller binds under
/// the same name (a type parameter, a local alias or class, a `typeof`
/// root), a generic callee's `T` is the argument's type, and an overloaded
/// callee's predicate is the selected overload's. A target no arm relates
/// to intersects the subject, printed as the checker prints it: the
/// undistributed `(A | B) & C` when distributing would grow the union,
/// else the distributed arms, a nullish arm dropping out beside the
/// object target under `strictNullChecks`. Measured on 7.0.2; only `o2`
/// and `o3` differ without `strictNullChecks`, where the nullish arms are
/// gone before the test.
#[test]
fn a_call_predicate_target_is_the_callees_instantiated_target() {
    check_rows(
        CALL_PREDICATE_SCOPES,
        &[
            ("g1", "number", "number"),
            ("g2", "number", "number"),
            ("g3", "number", "number"),
            ("g4", "1", "1"),
            ("g5", "1", "1"),
            ("g6", "string | number", "string | number"),
            (
                "g8",
                "(x: number | string) => number",
                "(x: number | string) => number",
            ),
            ("g9", "number", "number"),
            ("g10", "string", "string"),
            ("o1", "(Bar | Foo) & C", "(Bar | Foo) & C"),
            ("o2", "(Bar & C) | (Foo & C)", "(Bar | Foo) & C"),
            ("o3", "(Bar & C) | (Foo & C)", "(Bar | Foo) & C"),
            ("o4", "Foo & C", "Foo & C"),
        ],
    );
}

const NEVER_AT_RETURN: &str = r#"
export function p1(x: string | number, b: boolean) { if (typeof x === "string") throw 0; if (typeof x === "number") throw 0; return b; }
export function p2(x: string | number, b: boolean) { if (typeof x === "string") throw 0; return b; }
export function p3(x: string, b: boolean) { if (typeof x === "string") throw 0; return b; }
export function p4(b: boolean, x: string) { if (typeof x === "string") throw 0; return b; }
export function p5(x: string | number, y: unknown) { if (typeof x !== "string") throw 0; return typeof y === "number"; }
export function p6(x: "a", y: unknown) { if (x === "a") throw 0; return typeof y === "string"; }
export function p7(x: string, b: boolean) { if (typeof x === "string") return b; throw 0; }
export function p8(seed: string, b: boolean) { seed = "y"; return b; }
export function sig_p1() { return p1; }
export function sig_p2() { return p2; }
export function sig_p3() { return p3; }
export function sig_p4() { return p4; }
export function sig_p5() { return p5; }
export function sig_p6() { return p6; }
export function sig_p7() { return p7; }
"#;

/// The checker's inferred predicate reads EVERY parameter at the single
/// return, not only the ones the returned test names: a parameter the
/// body has already narrowed to `never` there is `never` on both edges,
/// so it is the predicate (`x is never`), and the first such parameter in
/// order wins over a named one (`p6`). A parameter narrowed short of
/// `never` (`p2`, `p5`'s `x`) and one read where it is not narrowed
/// (`p7`) establish nothing, and a reassigned parameter is never one
/// (`p8` is `boolean`, complete). Measured on 7.0.2, identically without
/// `strictNullChecks`.
#[test]
fn a_parameter_never_at_the_return_is_the_inferred_predicate() {
    let rows = [
        ("sig_p1", "(x: string | number, b: boolean) => x is never"),
        ("sig_p2", "(x: string | number, b: boolean) => boolean"),
        ("sig_p3", "(x: string, b: boolean) => x is never"),
        ("sig_p4", "(b: boolean, x: string) => x is never"),
        ("sig_p5", "(x: string | number, y: unknown) => y is number"),
        ("sig_p6", "(x: \"a\", y: unknown) => x is never"),
        ("sig_p7", "(x: string, b: boolean) => boolean"),
    ];
    let rows: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(function, printed)| (*function, *printed, *printed))
        .collect();
    check_predicate_rows(NEVER_AT_RETURN, &rows);
    check_rows(NEVER_AT_RETURN, &[("p8", "boolean", "boolean")]);
}

const INTERSECTION_READS: &str = r#"
export interface A { a: 1 }
export interface B { b: 1 }
export interface C { c: 1 }
export interface Foo { kind: 'foo'; n: number }
export interface Bar { kind: 'bar'; s: string }
export class Box { v: Foo | Bar = null!; isFoo(): this is { v: Foo } { return true; } }
type VF = { v: Foo };
export function r1(o: { v: A | B } & { v: A }) { return o.v; }
export function r2(o: { v: A | B } & { v: C }) { return o.v; }
export function r3(o: { v: Foo | Bar } & { v: Foo }) { return o.v; }
export function r4(o: { v: string | number } & { v: string }) { return o.v; }
export function r5(o: { v: A | B | C } & { v: C }) { return o.v; }
export function r6(o: { v: "a" | "b" } & { v: string }) { return o.v; }
export function r7(o: { v: Foo | Bar | null } & { v: Foo }) { return o.v; }
export function m1(b: Box) { if (b.isFoo()) return b; throw 0; }
export function m2(b: Box & VF) { return b.v; }
"#;

/// A member read through an intersection is the intersection of the
/// constituents' member types, which the checker DISTRIBUTES over a union
/// member type: a combination collapses on a conflicting discriminant, a
/// disjoint scalar or (under `strictNullChecks`) a nullish arm beside an
/// object type, a base primitive beside its literal adds nothing
/// (`"a" & string` is `"a"`), and the undistributed intersection stays
/// the print when distributing would grow it (`(A | B) & C`). Measured on
/// 7.0.2, identically without `strictNullChecks`.
#[test]
fn a_member_read_through_an_intersection_distributes_like_the_checker() {
    let rows = [
        ("r1", "A | (B & A)"),
        ("r2", "(A | B) & C"),
        ("r3", "Foo"),
        ("r4", "string"),
        ("r5", "(A | B | C) & C"),
        ("r6", "\"a\" | \"b\""),
        ("r7", "Foo"),
        ("m1", "Box & { v: Foo; }"),
        ("m2", "Foo"),
    ];
    let rows: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(function, printed)| (*function, *printed, *printed))
        .collect();
    check_rows(INTERSECTION_READS, &rows);
}

const CALL_INFERENCE: &str = r#"
export interface A { a: 1 }
export interface B { b: 1 }
export interface Base { b: 1 }
export interface Sub extends Base { s: 1 }
declare const u: ((x: unknown) => x is A) | ((x: unknown) => x is B);
declare const i: ((x: unknown) => x is A) & ((x: unknown) => x is B);
declare const iff: ((x: unknown) => x is A) & ((x: unknown) => false);
declare const ib: ((x: unknown) => boolean) & ((x: unknown) => x is B);
declare const us: ((x: unknown) => x is Sub) | ((x: unknown) => x is Base);
declare const ub: ((x: unknown) => x is Base) | ((x: unknown) => x is Sub);
declare function takes<S>(g: (x: unknown) => x is S): S;
declare function two<T>(a: T, b: T): T;
export function ti() { return takes(i); }
export function tiff() { return takes(iff); }
export function tib() { return takes(ib); }
export function tus() { return takes(us); }
export function tub() { return takes(ub); }
export function tlit() { return two("a", "b"); }
export function tnull(s: string | undefined, n: string) { return two(s, n); }
export function eu() { const v: typeof u extends (x: any) => x is A | B ? 1 : 0 = null!; return v; }
export function ei() { const v: typeof i extends (x: any) => x is A & B ? 1 : 0 = null!; return v; }
export function ei2() { const v: typeof i extends (x: any) => x is A ? 1 : 0 = null!; return v; }
export function aab() { const v: A extends A & B ? 1 : 0 = null!; return v; }
export function aba() { const v: A & B extends A ? 1 : 0 = null!; return v; }
export function pi() { const v: typeof i extends (x: any) => x is infer U ? U : never = null!; return v; }
export function piff() { const v: typeof iff extends (x: any) => x is infer U ? U : never = null!; return v; }
export function pib() { const v: typeof ib extends (x: any) => x is infer U ? U : never = null!; return v; }
"#;

/// Inference from an overloaded argument reads its LAST signature only
/// (`inferFromSignatures`): `takes(i)` is `B`, and `takes(iff)` infers
/// nothing from `(x: unknown) => false` — `S` is `unknown`, though an
/// earlier overload still makes the argument assignable — the same rule a
/// conditional type's `x is infer U` follows. A union argument deposits
/// one candidate per member, and a call's covariant candidates combine as
/// their COMMON SUPERTYPE (`getCommonSupertype`), never their union:
/// `takes(us)` over `Sub` and `Base` is `Base` in either order, literals
/// of one base union, and a nullable candidate keeps its `undefined`. A
/// conditional whose extends type is an intersection decides each member
/// (`A extends A & B` is `0`). Measured on 7.0.2, identically without
/// `strictNullChecks` except `tnull` (`string`).
#[test]
fn call_argument_inference_reads_the_last_overload_and_the_common_supertype() {
    check_rows(
        CALL_INFERENCE,
        &[
            ("ti", "B", "B"),
            ("tiff", "unknown", "unknown"),
            ("tib", "B", "B"),
            ("tus", "Base", "Base"),
            ("tub", "Base", "Base"),
            ("tlit", "\"a\" | \"b\"", "\"a\" | \"b\""),
            ("tnull", "string | undefined", "string"),
            ("eu", "1", "1"),
            ("ei", "0", "0"),
            ("ei2", "1", "1"),
            ("aab", "0", "0"),
            ("aba", "1", "1"),
            ("pi", "B", "B"),
            ("piff", "unknown", "unknown"),
            ("pib", "B", "B"),
        ],
    );
}

const NAMED_COMPOSITE_CONDITIONALS: &str = r#"
export interface A { a: 1 }
export interface B { b: 1 }
type TA = { a: 1 };
type TB = { b: 1 };
export interface Foo { kind: 'foo'; n: number }
export interface Bar { kind: 'bar'; s: string }
export function ab() { const v: A extends B ? 1 : 0 = null!; return v; }
export function aab() { const v: A extends A & B ? 1 : 0 = null!; return v; }
export function taab() { const v: TA extends TA & TB ? 1 : 0 = null!; return v; }
export function lab() { const v: { a: 1 } extends { a: 1 } & { b: 1 } ? 1 : 0 = null!; return v; }
export function fbu() { const v: Foo | Bar extends Foo ? 1 : 0 = null!; return v; }
export function fbu2() { const v: Foo | Bar extends Foo | Bar ? 1 : 0 = null!; return v; }
export function aba() { const v: A & B extends A ? 1 : 0 = null!; return v; }
"#;

/// A named arm of a union source or an intersection target relates
/// through the relation authority like any other arm, so a conditional
/// over such a composite decides: `A extends A & B` and `Foo | Bar extends
/// Foo` are `0`, the same spellings over type literals too. Measured on
/// 7.0.2, identically without `strictNullChecks`.
#[test]
fn a_conditional_over_named_composites_decides_like_the_checker() {
    let rows = [
        ("ab", "0"),
        ("aab", "0"),
        ("taab", "0"),
        ("lab", "0"),
        ("fbu", "0"),
        ("fbu2", "1"),
        ("aba", "1"),
    ];
    let rows: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|(function, printed)| (*function, *printed, *printed))
        .collect();
    check_rows(NAMED_COMPOSITE_CONDITIONALS, &rows);
}

const REFERENCE_EQUALITY: &str = r#"
export const K = "a" as const;
export function e1(x: "a" | "b", y: "a") { if (x === y) return x; throw 0; }
export function e1y(x: "a" | "b", y: "a") { if (x === y) return y; throw 0; }
export function e2(x: "a" | "b", y: "a") { if (x !== y) return x; throw 0; }
export function e2y(x: "a" | "b", y: "a") { if (x !== y) return y; throw 0; }
export function e3(x: "a" | "b", y: "a" | "c") { if (x === y) return x; throw 0; }
export function e3y(x: "a" | "b", y: "a" | "c") { if (x === y) return y; throw 0; }
export function e4(x: "a" | "b", y: "a" | "c") { if (x !== y) return x; throw 0; }
export function e5(x: string | number, y: string) { if (x === y) return x; throw 0; }
export function e6(x: string, y: "a" | "b") { if (x === y) return x; throw 0; }
export function e6n(x: string | number, y: "a" | 1) { if (x === y) return x; throw 0; }
export function e7(x: unknown, y: string) { if (x === y) return x; throw 0; }
export function e8(x: unknown, y: { a: 1 }) { if (x === y) return x; throw 0; }
export function e8b(x: unknown, y: "a" | { a: 1 }) { if (x === y) return x; throw 0; }
export function e9(x: string | null, y: null) { if (x === y) return x; throw 0; }
export function e10(x: string | null, y: null) { if (x !== y) return x; throw 0; }
export function e10u(x: string | null | undefined, y: null) { if (x != y) return x; throw 0; }
export function e11(x: string | undefined, y: string | undefined) { if (x === y) return x; throw 0; }
export function e12(x: number | string | boolean, y: number) { if (x == y) return x; throw 0; }
export function e12s(x: number | string | boolean, y: number) { if (x === y) return x; throw 0; }
export function e13(x: 1 | "1" | true | "x", y: 1) { if (x == y) return x; throw 0; }
export function e13n(x: 1 | "1" | true | "x", y: 1) { if (x != y) return x; throw 0; }
export function e14(x: any, y: string) { if (x === y) return x; throw 0; }
export function e15(x: string, y: any) { if (x === y) return x; throw 0; }
export function e16(x: "a" | "b") { const k = "a"; if (x === k) return x; throw 0; }
export function e17(x: "a" | "b") { const k = "a"; if (x !== k) return x; throw 0; }
export function e19(x: {}, y: string) { if (x === y) return x; throw 0; }
export function e21(x: "a", y: "a") { if (x !== y) return x; throw 0; }
export function e21y(x: "a", y: "a") { if (x !== y) return y; throw 0; }
export function e22(x: "a" | "b", y: "a" | "b") { if (x !== y) return x; throw 0; }
export function e23x(x: 1 | 2, y: 2 | 3) { if (x === y) return x; throw 0; }
export function e23y(x: 1 | 2, y: 2 | 3) { if (x === y) return y; throw 0; }
export function e24(x: boolean, y: true) { if (x === y) return x; throw 0; }
export function e24n(x: boolean, y: true) { if (x !== y) return x; throw 0; }
export function e25(x: "a" | "b", y: "a" | "b") { if (y === x) return x; throw 0; }
export function e26(x: "a" | "b" | undefined, y: undefined) { if (x == y) return x; throw 0; }
export function k1(x: "a" | "b") { let k = "a"; if (x === k) return x; throw 0; }
export function k2(x: "a" | "b") { if (x === K) return x; throw 0; }
export function k3(x: "a" | "b") { if (x !== K) return x; throw 0; }
export function o1(x: { a: 1 } | { b: 1 }, y: { a: 1 }) { if (x === y) return x; throw 0; }
export function o2(x: { a: 1 } | string, y: { a: 1 }) { if (x === y) return x; throw 0; }
export function o3(x: { a: 1 } | string, y: { a: 1 }) { if (x !== y) return x; throw 0; }
export function u1(x: unknown, y: "a" | "b") { if (x === y) return x; throw 0; }
export function u2(x: unknown, y: boolean) { if (x === y) return { v: x }; throw 0; }
export function u3(x: unknown, y: unknown) { if (x === y) return x; throw 0; }
export function u4(x: unknown, y: string) { if (x == y) return x; throw 0; }
export function l2(x: "1" | 1 | true) { if (x == 1) return x; throw 0; }
export function l3(x: string | number | boolean, y: string) { if (x == y) return x; throw 0; }
export function z1(x: "a" | "b") { if (x !== Missing) return x; throw 0; }
export function p1(x: "a" | "b", y: "a") { return x === y; }
export function p2(x: string, y: string) { return x === y; }
export function p3(x: unknown, y: string) { return x === y; }
export function p4(y: "a", x: "a" | "b") { return x === y; }
export function sig_p1() { return p1; }
export function sig_p2() { return p2; }
export function sig_p3() { return p3; }
export function sig_p4() { return p4; }
"#;

/// An equality between two VALUES narrows every reference operand by the
/// other operand's type at the test (`narrowTypeByEquality`), both types
/// read before either narrow applies: `x: "a"` and `y: "a"` are both
/// `never` past `x !== y`. On the equal edge the arms comparable to the
/// value survive (`==` also keeps a `number` / `string` / boolean-literal
/// arm against a `number` / `string` / `boolean` value, and selects both
/// nullish arms against a nullish value), a surviving `string` / `number`
/// / `bigint` arm becomes the value's literals of its kind, and an
/// `unknown` or `{}` subject of `===` IS a primitive value (`object` for an
/// object value, unchanged for a union); on the unequal edge only a unit
/// value narrows. A `let` operand reads its declared `string`; a module
/// constant, its literal. A returned equality infers the predicate the
/// true edge establishes when the false edge empties it (`p1`, `p4`). A
/// value this substrate cannot resolve (`Missing`) is no fact about either
/// edge: the narrow degrades.
/// Measured on 7.0.2 with `--strict` and with `--strictNullChecks false`
/// (`e9` reads `string`, `e11` `string` and `e26` `"a" | "b"` there).
#[test]
fn an_equality_between_two_values_narrows_both_references() {
    let rows = [
        ("e1", "\"a\"", "\"a\""),
        ("e1y", "\"a\"", "\"a\""),
        ("e2", "\"b\"", "\"b\""),
        ("e2y", "\"a\"", "\"a\""),
        ("e3", "\"a\"", "\"a\""),
        ("e3y", "\"a\"", "\"a\""),
        ("e4", "\"a\" | \"b\"", "\"a\" | \"b\""),
        ("e5", "string", "string"),
        ("e6", "\"a\" | \"b\"", "\"a\" | \"b\""),
        ("e6n", "\"a\" | 1", "\"a\" | 1"),
        ("e7", "string", "string"),
        ("e8", "object", "object"),
        ("e8b", "unknown", "unknown"),
        ("e9", "null", "string"),
        ("e10", "string", "string"),
        ("e10u", "string", "string"),
        ("e11", "string | undefined", "string"),
        (
            "e12",
            "string | number | boolean",
            "string | number | boolean",
        ),
        ("e12s", "number", "number"),
        ("e13", "1", "1"),
        ("e13n", "\"1\" | \"x\" | true", "\"1\" | \"x\" | true"),
        ("e14", "any", "any"),
        ("e15", "string", "string"),
        ("e16", "\"a\"", "\"a\""),
        ("e17", "\"b\"", "\"b\""),
        ("e19", "string", "string"),
        ("e21", "never", "never"),
        ("e21y", "never", "never"),
        ("e22", "\"a\" | \"b\"", "\"a\" | \"b\""),
        ("e23x", "2", "2"),
        ("e23y", "2", "2"),
        ("e24", "true", "true"),
        ("e24n", "false", "false"),
        ("e25", "\"a\" | \"b\"", "\"a\" | \"b\""),
        ("e26", "undefined", "\"a\" | \"b\""),
        ("k1", "\"a\" | \"b\"", "\"a\" | \"b\""),
        ("k2", "\"a\"", "\"a\""),
        ("k3", "\"b\"", "\"b\""),
        ("o1", "{ a: 1; }", "{ a: 1; }"),
        ("o2", "{ a: 1; }", "{ a: 1; }"),
        ("o3", "string | { a: 1; }", "string | { a: 1; }"),
        ("u1", "unknown", "unknown"),
        ("u2", "{ v: boolean; }", "{ v: boolean; }"),
        ("u3", "unknown", "unknown"),
        ("u4", "unknown", "unknown"),
        ("l2", "1", "1"),
        (
            "l3",
            "string | number | boolean",
            "string | number | boolean",
        ),
    ];
    check_rows(REFERENCE_EQUALITY, &rows);
    let host = host_with(REFERENCE_EQUALITY);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        super::signature_predicate_inference_tests::assert_degrades(&host, root, "z1");
    }
    check_predicate_rows(
        REFERENCE_EQUALITY,
        &[
            (
                "sig_p1",
                "(x: \"a\" | \"b\", y: \"a\") => x is \"a\"",
                "(x: \"a\" | \"b\", y: \"a\") => x is \"a\"",
            ),
            (
                "sig_p2",
                "(x: string, y: string) => boolean",
                "(x: string, y: string) => boolean",
            ),
            (
                "sig_p3",
                "(x: unknown, y: string) => boolean",
                "(x: unknown, y: string) => boolean",
            ),
            (
                "sig_p4",
                "(y: \"a\", x: \"a\" | \"b\") => x is \"a\"",
                "(y: \"a\", x: \"a\" | \"b\") => x is \"a\"",
            ),
        ],
    );
}

const CALL_RECOVERY: &str = r#"
export interface A { a: 1 }
export interface B { b: 1 }
declare const u: ((x: unknown) => x is A) | ((x: unknown) => x is B);
declare function takes<S>(g: (x: unknown) => x is S): S;
declare function two<T>(a: T, b: T): T;
declare function one(x: string): number;
declare function rest(a: string, ...more: number[]): number;
declare function opt(a: string, b?: number): number;
declare function con<T extends string>(x: T): T;
declare function ctx<T>(x: T, f: (v: T) => void): T;
declare function box<T>(x: T[]): T;
declare function ov(x: string): string;
declare function ov(x: number): number;
declare function cb(f: () => number): void;
declare function g2<T>(x: T): T;
declare function g2<T>(x: T, y: T): T;
declare const o: { m(x: string): number };
export function tu() { return takes(u); }
export function tab(x: { a: 1 }, y: { b: 1 }) { return two(x, y); }
export function o1(x: number) { return one(x); }
export function o2() { return one(); }
export function o3() { return one("a", "b"); }
export function r0() { return rest(); }
export function op0() { return opt(); }
export function c1() { return con(1); }
export function x1() { return ctx(1, (v: string) => {}); }
export function bx(x: number) { return box(x); }
export function v1(x: boolean) { return ov(x); }
export function e1() { return cb(() => "s"); }
export function eg() { return g2<string>(1); }
export function eg1() { return con<"a">("b"); }
export function cc() { return [con(1), con(true)]; }
export function c3() { return con(1); }
export function om() { return o.m(); }
declare function ov3(f: (x: string) => string): void;
declare function ov3(f: (x: string) => string, b: string, c: string): void;
declare function ovc(f: (x: string) => number, n: number): void;
declare function ovc(f: (x: number) => boolean, n: string): void;
declare function lit(x: { a: number; b: string }): void;
declare function lit2(x: { color: number }): void;
declare function nested(x: { a: { b: number } }): void;
declare const ob2: { a: { b: string } };
declare function arr(x: number[]): void;
declare function cbu(f: (() => number) | ((x: string) => boolean)): void;
declare function two2(a: number, b: number): number;
export function gap() { return ov3((x) => x, "b"); }
export function lo() { return ovc((x) => "s", "n"); }
export function lo2() { return ovc((x) => x, true); }
export function el() { return lit({ a: "x", b: 1 }); }
export function ex() { return lit({ a: 1, b: "x", c: 1 }); }
export function sg() { return lit2({ colour: 1 }); }
export function np() { return nested(ob2); }
export function ae() { return arr(["a"]); }
export function ur() { return cbu(() => "s"); }
export function cbb() { return cb(() => { return "s"; }); }
export function ts1() { return two2(...[1, "bad"] as const); }
export function ts2() { return one(...([] as const), 1); }
export function ts3() { return one(...["a", "b"] as const); }
export function un1() { enum Local { K = 1 } return one(Local); }
declare enum E { A, B }
declare function sb(x: string | boolean): void;
export function en() { return one(E.A); }
export function bu() { return sb(1); }
declare function fe(x: E): void;
type AB = "a" | "b";
declare function fab(x: AB): void;
export function fe_s() { return fe("s"); }
export function fab_s() { return fab("c"); }
export function fab_n() { return fab(1); }
export function wobj() { return one({ a: 1 }); }
export function wobj2() { return one({ a: 1, b: { c: true } }); }
export function warr() { return one([1]); }
export function wl() { return lit({ a: 1 }); }
declare function rnd(): boolean;
export function lp1() { let x: string | number = 1; for (let i = 0; i < 3; i++) { if (rnd()) { return one(x); } x = "a"; } return 0; }
"#;

/// A call its only candidate does not accept continues with that
/// candidate as the checker's error-recovery candidate
/// (`getCandidateForOverloadFailure`), re-inferred from the arguments.
/// Measured on 7.0.2, identically without `strictNullChecks`: `takes(u)`
/// is `A`; `two(x, y)` over `{ a: 1 }` and `{ b: 1 }` is `{ a: 1 }` — the
/// candidates' common supertype, which `y` then fails; `con(1)` is
/// `string` — the inference violating `T extends string` takes the
/// constraint; `ctx(1, (v: string) => {})` is `string`. With explicit
/// type arguments over overloads, the one candidate whose arity fits the
/// call recovers it: `g2<string>(1)` is `string`.
///
/// A call to a lone NON-generic signature is answered from its declared
/// return whatever the arguments: `one(x)`, `one()`, `one("a", "b")`,
/// `rest()` and `opt()` are `number`.
///
/// A generic candidate whose inference relation fails (`box(x)`, the
/// checker's `unknown`) and a call over several overloads (`ov(x)`, the
/// checker's `never`) keep the typed call gap: the first re-infers from a
/// relation this executor abandoned, the second answers from a signature
/// combining every overload.
#[test]
fn a_call_its_only_candidate_rejects_answers_the_checkers_recovery() {
    let rows = [
        ("tu", "A"),
        ("tab", "{ a: 1; }"),
        ("c1", "string"),
        ("x1", "string"),
        ("eg", "string"),
    ];
    let host = host_with(CALL_RECOVERY);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, printed) in rows {
            assert_prints(&host, root, function, printed);
        }
        for function in ["o1", "o2", "o3", "r0", "op0"] {
            assert_prints(&host, root, function, "number");
        }
        for function in ["bx", "v1"] {
            super::signature_predicate_inference_tests::assert_degrades(&host, root, function);
        }
    }
}

/// The diagnostics riding each function's answer, through the public
/// audited flow-return boundary, printed as `tsc --pretty false` prints
/// each — `(line,col): error TS<code>: <message>`, then its message chain
/// indented two spaces per level — in its order. Every expected line is
/// TypeScript 7.0.2's on this fixture, identical with `strictNullChecks`
/// off.
///
/// The positions are the checker's: an argument's start for TS2345 (a
/// spread's `...` for every argument it expands to), the first argument
/// past the candidates' arity for too many arguments, the callee — a
/// property access's NAME — for too few and for TS2575, and the part of an
/// authored argument the failure is elaborated into: an arrow callback's
/// returned expression, an object literal's property name, an array
/// literal's element. TS2769 is reported where its last overload's error
/// is, with that error below it — the last overload's error names a
/// context-sensitive callback as that overload's own parameters typed it
/// (`lo2`, whose later `true` the same overloads also reject). A literal
/// argument prints as its base primitive, and an enum member as its enum,
/// unless the parameter could hold a literal (`"b"` against `"a"`, `1`
/// against `string | boolean`, whose `boolean` the checker holds as
/// `true | false`; an enum or aliased literal-union parameter reads
/// through what it settles to, keeping `"s"` against `E` and `"c"` against
/// `AB`). A fresh object or array literal argument prints as its widened
/// literal tree (`{ a: number; }`, `number[]`, and the missing-property
/// message's source likewise), and a union prints its members in the
/// checker's order (`string | number`, the loop read `lp1` narrows to, is
/// reported once — a provisional loop-head pass's occurrences are set
/// aside with its other side outputs).
///
/// Two calls of one callee with equal arguments (`con(1)` in `c1` and
/// `c3`) each report at their own call, and two failing calls of one body
/// (`cc`) report in source order; a warm read serves the same occurrences.
#[test]
fn call_diagnostics_carry_the_checkers_code_message_position_and_order() {
    let rows: &[(&str, &[&str])] = &[
        ("tu", &["(19,37): error TS2345: Argument of type '((x: unknown) => x is A) | ((x: unknown) => x is B)' is not assignable to parameter of type '(x: unknown) => x is A'.\n  Type '(x: unknown) => x is B' is not assignable to type '(x: unknown) => x is A'.\n    Type predicate 'x is B' is not assignable to 'x is A'.\n      Property 'a' is missing in type 'B' but required in type 'A'."]),
        ("tab", &["(20,63): error TS2741: Property 'a' is missing in type '{ b: 1; }' but required in type '{ a: 1; }'."]),
        ("o1", &["(21,44): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("o2", &["(22,31): error TS2554: Expected 1 arguments, but got 0."]),
        ("o3", &["(23,40): error TS2554: Expected 1 arguments, but got 2."]),
        ("r0", &["(24,31): error TS2555: Expected at least 1 arguments, but got 0."]),
        ("op0", &["(25,32): error TS2554: Expected 1-2 arguments, but got 0."]),
        ("c1", &["(26,35): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("x1", &["(27,35): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("e1", &["(30,40): error TS2322: Type 'string' is not assignable to type 'number'."]),
        ("eg", &["(31,42): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("eg1", &["(32,41): error TS2345: Argument of type '\"b\"' is not assignable to parameter of type '\"a\"'."]),
        (
            "cc",
            &[
                "(33,36): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'.",
                "(33,44): error TS2345: Argument of type 'boolean' is not assignable to parameter of type 'string'.",
            ],
        ),
        ("c3", &["(34,35): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("om", &["(35,33): error TS2554: Expected 1 arguments, but got 0."]),
        ("gap", &["(47,32): error TS2575: No overload expects 2 arguments, but overloads do exist that expect either 1 or 3 arguments."]),
        ("lo", &["(48,42): error TS2769: No overload matches this call.\n  The last overload gave the following error.\n    Type 'string' is not assignable to type 'boolean'."]),
        ("lo2", &["(49,43): error TS2769: No overload matches this call.\n  The last overload gave the following error.\n    Type 'number' is not assignable to type 'boolean'."]),
        (
            "el",
            &[
                "(50,37): error TS2322: Type 'string' is not assignable to type 'number'.",
                "(50,45): error TS2322: Type 'number' is not assignable to type 'string'.",
            ],
        ),
        ("ex", &["(51,51): error TS2353: Object literal may only specify known properties, and 'c' does not exist in type '{ a: number; b: string; }'."]),
        ("sg", &["(52,38): error TS2561: Object literal may only specify known properties, but 'colour' does not exist in type '{ color: number; }'. Did you mean to write 'color'?"]),
        ("np", &["(53,38): error TS2345: Argument of type '{ a: { b: string; }; }' is not assignable to parameter of type '{ a: { b: number; }; }'.\n  The types of 'a.b' are incompatible between these types.\n    Type 'string' is not assignable to type 'number'."]),
        ("ae", &["(54,36): error TS2322: Type 'string' is not assignable to type 'number'."]),
        ("ur", &["(55,41): error TS2322: Type 'string' is not assignable to type 'number | boolean'."]),
        ("cbb", &["(56,35): error TS2345: Argument of type '() => string' is not assignable to parameter of type '() => number'.\n  Type 'string' is not assignable to type 'number'."]),
        ("ts1", &["(57,37): error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'."]),
        ("ts2", &["(58,54): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."]),
        ("ts3", &["(59,36): error TS2554: Expected 1 arguments, but got 2."]),
        ("en", &["(63,35): error TS2345: Argument of type 'E' is not assignable to parameter of type 'string'."]),
        ("bu", &["(64,34): error TS2345: Argument of type '1' is not assignable to parameter of type 'string | boolean'."]),
        ("fe_s", &["(68,36): error TS2345: Argument of type '\"s\"' is not assignable to parameter of type 'E'."]),
        ("fab_s", &["(69,38): error TS2345: Argument of type '\"c\"' is not assignable to parameter of type 'AB'."]),
        ("fab_n", &["(70,38): error TS2345: Argument of type '1' is not assignable to parameter of type 'AB'."]),
        ("wobj", &["(71,37): error TS2345: Argument of type '{ a: number; }' is not assignable to parameter of type 'string'."]),
        ("wobj2", &["(72,38): error TS2345: Argument of type '{ a: number; b: { c: boolean; }; }' is not assignable to parameter of type 'string'."]),
        ("warr", &["(73,37): error TS2345: Argument of type 'number[]' is not assignable to parameter of type 'string'."]),
        ("wl", &["(74,35): error TS2741: Property 'b' is missing in type '{ a: number; }' but required in type '{ a: number; b: string; }'."]),
        ("lp1", &["(76,107): error TS2345: Argument of type 'string | number' is not assignable to parameter of type 'string'.\n  Type 'number' is not assignable to type 'string'."]),
    ];
    let host = host_with(CALL_RECOVERY);
    let mut mismatches = Vec::new();
    // Cold, then warm: a warm read serves the same occurrences.
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (function, expected) in rows {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host,
                    root,
                    function,
                    CALL_RECOVERY,
                );
                if reported != *expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// Declarations and calls whose messages depend on how the checker orders,
/// prints and elaborates what an optional position or a nullable union holds.
const CALL_REPORT_FORMS: &str = r#"
declare function one(x: string): number;
declare function obj(x: { a?: number }): void;
declare function g3<T>(x: T, y: T, z: T): T;
declare function g3<T>(x: T): T;
declare function op2(x?: string): void;
declare function opu(x?: string | undefined): void;
declare const nu: number | undefined;
declare const nl: null | number;
declare const bu2: boolean | undefined;
declare const bs: boolean | symbol;
declare function bsf(x: string | number): void;
declare const mix: true | 1 | "a";
declare function big(x: bigint): void;
export function optional() { return obj({ a: "bad" }); }
export function optional_undefined() { return obj({ a: undefined }); }
export function reversed_explicit() { return g3<string>(1); }
export function optional_param() { return op2(1); }
export function optional_param_undefined() { return opu(1); }
export function nullish_number() { return one(nu); }
export function null_number() { return one(nl); }
export function boolean_undefined() { return one(bu2); }
export function boolean_symbol() { return bsf(bs); }
export function mixed_literals() { return big(mix); }
declare function nlp(x: number | undefined): void;
export function nullable_target() { return nlp("a"); }
declare function nn(x: number): void;
declare const au: "a" | undefined;
declare const an: "a" | "b" | null;
declare const tu: true | undefined;
declare const e2: 1 | 2 | undefined;
export function literal_undefined() { return nn(au); }
export function literals_null() { return nn(an); }
export function true_undefined() { return nn(tu); }
export function numbers_undefined() { return nn(e2); }
export function nested() { return one(one()); }
declare function ns(x: string): void;
declare const bl: boolean | "a";
declare const bn: boolean | 1 | null;
declare enum E { A, B }
declare const eb: E.A | boolean;
declare const w: { a?: string };
declare function objm(x: { a: string; b?: number }): void;
declare const sym: unique symbol;
declare function us(x: typeof sym): void;
declare function po(x: { a?: number }): void;
declare const wo: { c: number };
export function bool_str() { return nn(bl); }
export function bool_num_null() { return nn(bn); }
export function bool_enum() { return ns(eb); }
export function optional_member_read() { return one(w); }
export function optional_member_missing() { return objm({}); }
export function unique_symbol() { return us("a"); }
export function weak_primitive() { return po(1); }
export function weak_object() { return po(wo); }
const af: (x: number) => string = () => "";
export function annotated_callee() { return af("x"); }
declare const um: number | { a: 1 };
export function weak_member() { return po(um); }
declare function pn(x: { o: { a?: number } }): void;
declare const on: { o: number };
export function weak_nested() { return pn(on); }
declare function gd<T>(x: T, y: T, z: T): number;
declare function gd<T>(x: T): string;
export function differing_return() { return gd<string>(1); }
"#;

/// An optional target property or parameter reports against its declared
/// type (`Type 'string' is not assignable to type 'number'.`), an overload
/// set under explicit type arguments reports its one arity-fitting
/// candidate's argument error and continues with the candidate the checker
/// recovers with, its first taking as many parameters as the call has
/// arguments (`getLongestCandidateIndex`): `gd<string>(1)` is `number`, that
/// candidate's return, beside the fitting one's TS2345. A union is related
/// and printed in the checker's own order, and `strictNullChecks: false`
/// leaves `null` and `undefined` out of every message. Measured on
/// TypeScript 7.0.2.
#[test]
fn call_diagnostics_follow_the_checkers_optionality_union_order_and_nullability() {
    const ARG: &str = "is not assignable to parameter of type";
    let argument =
        |source: &str, target: &str| format!("Argument of type '{source}' {ARG} '{target}'.");
    let rows: Vec<(&str, &str, Vec<String>, Vec<String>)> = {
        let number_string = vec![format!("error TS2345: {}", argument("number", "string"))];
        vec![
            (
                "optional",
                "(15,43)",
                vec!["error TS2322: Type 'string' is not assignable to type 'number'.".into()],
                vec!["error TS2322: Type 'string' is not assignable to type 'number'.".into()],
            ),
            ("optional_undefined", "", vec![], vec![]),
            (
                "nested",
                "(36,39)",
                vec![
                    "error TS2554: Expected 1 arguments, but got 0.".into(),
                    format!("error TS2345: {}", argument("number", "string")),
                ],
                vec![
                    "error TS2554: Expected 1 arguments, but got 0.".into(),
                    format!("error TS2345: {}", argument("number", "string")),
                ],
            ),
            (
                "reversed_explicit",
                "(17,57)",
                number_string.clone(),
                number_string.clone(),
            ),
            (
                "differing_return",
                "(65,56)",
                number_string.clone(),
                number_string.clone(),
            ),
            (
                "optional_param",
                "(18,47)",
                number_string.clone(),
                number_string.clone(),
            ),
            (
                "optional_param_undefined",
                "(19,57)",
                number_string.clone(),
                number_string.clone(),
            ),
        ]
    };
    let nullable: &[(&str, &str, &[&str], &[&str])] = &[
        (
            "nullish_number",
            "(20,47)",
            &["error TS2345: Argument of type 'number | undefined' is not assignable to parameter of type 'string'.\n  Type 'undefined' is not assignable to type 'string'."],
            &["error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."],
        ),
        (
            "null_number",
            "(21,44)",
            &["error TS2345: Argument of type 'number | null' is not assignable to parameter of type 'string'.\n  Type 'null' is not assignable to type 'string'."],
            &["error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'."],
        ),
        (
            "boolean_undefined",
            "(22,50)",
            &["error TS2345: Argument of type 'boolean | undefined' is not assignable to parameter of type 'string'.\n  Type 'undefined' is not assignable to type 'string'."],
            &["error TS2345: Argument of type 'boolean' is not assignable to parameter of type 'string'."],
        ),
        (
            "boolean_symbol",
            "(23,47)",
            &["error TS2345: Argument of type 'symbol | boolean' is not assignable to parameter of type 'string | number'.\n  Type 'symbol' is not assignable to type 'string | number'."],
            &["error TS2345: Argument of type 'symbol | boolean' is not assignable to parameter of type 'string | number'.\n  Type 'symbol' is not assignable to type 'string | number'."],
        ),
        (
            "literal_undefined",
            "(32,49)",
            &["error TS2345: Argument of type 'string | undefined' is not assignable to parameter of type 'number'.\n  Type 'undefined' is not assignable to type 'number'."],
            &["error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'."],
        ),
        (
            "literals_null",
            "(33,45)",
            &["error TS2345: Argument of type 'string | null' is not assignable to parameter of type 'number'.\n  Type 'null' is not assignable to type 'number'."],
            &["error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'.\n  Type 'string' is not assignable to type 'number'."],
        ),
        (
            "true_undefined",
            "(34,46)",
            &["error TS2345: Argument of type 'boolean | undefined' is not assignable to parameter of type 'number'.\n  Type 'undefined' is not assignable to type 'number'."],
            &["error TS2345: Argument of type 'boolean' is not assignable to parameter of type 'number'."],
        ),
        (
            "numbers_undefined",
            "(35,49)",
            &["error TS2345: Argument of type 'number | undefined' is not assignable to parameter of type 'number'.\n  Type 'undefined' is not assignable to type 'number'."],
            &[],
        ),
        (
            "bool_str",
            "(48,40)",
            &["error TS2345: Argument of type 'string | boolean' is not assignable to parameter of type 'number'.\n  Type 'string' is not assignable to type 'number'."],
            &["error TS2345: Argument of type 'string | boolean' is not assignable to parameter of type 'number'.\n  Type 'string' is not assignable to type 'number'."],
        ),
        (
            "bool_num_null",
            "(49,45)",
            &["error TS2345: Argument of type 'number | boolean | null' is not assignable to parameter of type 'number'.\n  Type 'null' is not assignable to type 'number'."],
            &["error TS2345: Argument of type 'number | boolean' is not assignable to parameter of type 'number'.\n  Type 'boolean' is not assignable to type 'number'."],
        ),
        (
            "bool_enum",
            "(50,41)",
            &["error TS2345: Argument of type 'boolean | E' is not assignable to parameter of type 'string'.\n  Type 'boolean' is not assignable to type 'string'."],
            &["error TS2345: Argument of type 'boolean | E' is not assignable to parameter of type 'string'.\n  Type 'boolean' is not assignable to type 'string'."],
        ),
        (
            "optional_member_read",
            "(51,53)",
            &["error TS2345: Argument of type '{ a?: string | undefined; }' is not assignable to parameter of type 'string'."],
            &["error TS2345: Argument of type '{ a?: string; }' is not assignable to parameter of type 'string'."],
        ),
        (
            "optional_member_missing",
            "(52,57)",
            &["error TS2741: Property 'a' is missing in type '{}' but required in type '{ a: string; b?: number | undefined; }'."],
            &["error TS2741: Property 'a' is missing in type '{}' but required in type '{ a: string; b?: number; }'."],
        ),
        (
            "unique_symbol",
            "(53,45)",
            &["error TS2345: Argument of type '\"a\"' is not assignable to parameter of type 'unique symbol'."],
            &["error TS2345: Argument of type '\"a\"' is not assignable to parameter of type 'unique symbol'."],
        ),
        // A primitive's apparent members are not modelled, so the weak-type
        // diagnostic (TS2559) is not reported rather than reported as TS2345.
        ("weak_primitive", "(54,43)", &[], &[]),
        // The relation does not model the weak-type rule for an object source, so
        // the checker's TS2559 is not reported at all.
        ("weak_object", "(55,43)", &[], &[]),
        // Below the head the checker explains a weak-type failure with TS2559's
        // text, which the explanation chain cannot reproduce for a primitive:
        // the whole diagnostic is withheld rather than reported with a wrong
        // chain line.
        ("weak_member", "(59,43)", &[], &[]),
        ("weak_nested", "(62,43)", &[], &[]),
        (
            "annotated_callee",
            "(57,48)",
            &["error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'."],
            &["error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'."],
        ),
        (
            "mixed_literals",
            "(24,47)",
            &["error TS2345: Argument of type 'string | number | boolean' is not assignable to parameter of type 'bigint'.\n  Type 'string' is not assignable to type 'bigint'."],
            &["error TS2345: Argument of type 'string | number | boolean' is not assignable to parameter of type 'bigint'.\n  Type 'string' is not assignable to type 'bigint'."],
        ),
    ];
    let host = host_with(CALL_REPORT_FORMS);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for (root, strict) in [(STRICT_ROOT, true), (LOOSE_ROOT, false)] {
            let mut check = |function: &str, position: &str, expected: Vec<String>| {
                let expected: Vec<String> = expected
                    .into_iter()
                    .map(|message| format!("{position}: {message}"))
                    .collect();
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host,
                    root,
                    function,
                    CALL_REPORT_FORMS,
                );
                if reported != expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            };
            for (function, position, strict_rows, loose_rows) in &rows {
                let expected = if strict { strict_rows } else { loose_rows };
                check(function, position, expected.clone());
            }
            for (function, position, strict_rows, loose_rows) in nullable {
                let expected = if strict { strict_rows } else { loose_rows };
                check(
                    function,
                    position,
                    expected.iter().map(|row| (*row).to_owned()).collect(),
                );
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        assert_prints(&host, root, "reversed_explicit", "string");
        assert_prints(&host, root, "differing_return", "number");
    }
}

const CALL_REPORT_IDENTITY: &str = r#"declare function assertNever(x: never): never;
export function f2() { return assertNever("a"); }
declare function f(x?: string, cb?: (n: number) => number): void;
export function f3() { return f(123, n => n); }
declare function g(o: { "1": string; other: number }): void;
declare const s: { 1: string };
export function f4() { return g(s); }
"#;

/// A literal keeps its spelling against `never`, an eagerly rejected
/// argument ahead of a callback is reported against the declared parameter
/// type without the optionality's `undefined`, and a numeric key and its
/// string spelling are one property. Measured on TypeScript 7.0.2.
#[test]
fn call_reports_keep_never_literals_optional_targets_and_numeric_keys() {
    let host = host_with(CALL_REPORT_IDENTITY);
    let checker_report = super::signature_predicate_inference_tests::checker_report;
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, expected) in [
            (
                "f2",
                "(2,43): error TS2345: Argument of type '\"a\"' is not assignable to parameter of type 'never'.",
            ),
            (
                "f3",
                "(4,33): error TS2345: Argument of type 'number' is not assignable to parameter of type 'string'.",
            ),
            ("f4", "(7,33): error TS2741: Property 'other' is missing in type"),
        ] {
            let reported = checker_report(&host, root, function, CALL_REPORT_IDENTITY);
            // Only the message's leading text is compared: how a numeric key
            // spells inside a printed object type is a separate printer concern.
            assert!(
                reported.len() == 1 && reported[0].starts_with(expected),
                "`{function}` in {root}: {reported:#?}"
            );
        }
    }
}

/// A long message prints as the checker prints it. Past its node builder's
/// truncation length a union's remaining members elide as `... N more ...`
/// before its last, a member it begins past that length as `{ ...; }`, and
/// an object's remaining properties as `... N more ...;` before its last; a
/// printed type of twice that length is clipped to its first 317 bytes and
/// `...`; and a failure through a property path of 70 nested object types
/// is explained on one path line, never cut. A reference's type arguments
/// are counted before its name, a tuple label adds nothing, an index
/// signature counts its authored parameter name, and a mapped type that is
/// not generic counts as the object type it resolves to, so each is elided exactly
/// where the checker elides it, and never withheld. Each is reported at its own
/// call, two calls of one body in source order, cold and warm. Every
/// expected line is TypeScript 7.0.2's on this fixture, identical with
/// `strictNullChecks` off.
#[test]
fn long_messages_print_as_the_checker_abbreviates_them() {
    let arms: Vec<String> = (0..40)
        .map(|ordinal| format!("{{ member{ordinal}: {ordinal} }}"))
        .collect();
    let properties: Vec<String> = (0..40)
        .map(|ordinal| format!("p{ordinal}: {ordinal}"))
        .collect();
    let nest = |inner: &str, depth: usize| {
        let mut ty = inner.to_owned();
        for _ in 0..depth {
            ty = format!("{{ a: {ty} }}");
        }
        ty
    };
    let source = format!(
        "\ndeclare function one(x: string): number;\n\
         declare function nested(x: {}): void;\n\
         declare const wide: {};\n\
         declare const deep: {};\n\
         declare const wobj: {{ {} }};\n\
         export function wide_union() {{ return one(wide); }}\n\
         export function wide_object() {{ return one(wobj); }}\n\
         export function deep_chain() {{ return nested(deep); }}\n\
         export function both() {{ return [one(wobj), one(wide)]; }}\n\
         export function short() {{ return one(1); }}\n\
         interface Wrapper<T> {{ w: T }}\n\
         declare const near: {{ a: \"{near}\"; b: Wrapper<string>; c: number }};\n\
         declare const past: {{ a: \"{past}\"; b: Wrapper<string>; c: number }};\n\
         declare const index_near: {{ t: {{ [k: string]: \"{index_near}\" }}; b: Wrapper<string> }};\n\
         declare const index_past: {{ t: {{ [k: string]: \"{index_past}\" }}; b: Wrapper<string> }};\n\
         declare const labelled: [{label}: string];\n\
         declare const indexed: {{ [k: string]: {{ {indexed} }} }};\n\
         export function references() {{ return [one(near), one(past)]; }}\n\
         export function index_counts() {{ return [one(index_near), one(index_past)]; }}\n\
         export function labelled_tuple() {{ return one(labelled); }}\n\
         export function index_signature() {{ return one(indexed); }}\n\
         declare const mapped: {{ [P in string]: {{ {indexed} }} }};\n\
         export function mapped_index() {{ return one(mapped); }}\n",
        nest("number", 70),
        arms.join(" | "),
        nest("string", 70),
        properties.join("; "),
        near = "x".repeat(150),
        past = "x".repeat(155),
        index_near = "x".repeat(138),
        index_past = "x".repeat(139),
        label = "a".repeat(90),
        indexed = properties[..20].join("; "),
    );
    const ARG: &str = "is not assignable to parameter of type 'string'.";
    let union = format!(
        "Argument of type '{} | {{ ...; }} | ... 26 more ... | {{ ...; }}' {ARG}\n  \
         Type '{{ member0: 0; }}' is not assignable to type 'string'.",
        (0..12)
            .map(|ordinal| format!("{{ member{ordinal}: {ordinal}; }}"))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    let object = format!(
        "Argument of type '{{ {}; ... 8 more ...; p39: 39; }}' {ARG}",
        properties[..31].join("; "),
    );
    let clipped = format!("{}{{ ...", "{ a: ".repeat(63));
    let deep = format!(
        "Argument of type '{clipped}' is not assignable to parameter of type '{clipped}'.\n  \
         The types of '{}' are incompatible between these types.\n    \
         Type 'string' is not assignable to type 'number'.",
        vec!["a"; 70].join("."),
    );
    let at = |position: &str, message: &str| format!("{position}: error TS2345: {message}");
    let rows: Vec<(&str, Vec<String>)> = vec![
        ("wide_union", vec![at("(7,43)", &union)]),
        ("wide_object", vec![at("(8,44)", &object)]),
        ("deep_chain", vec![at("(9,46)", &deep)]),
        ("both", vec![at("(10,38)", &object), at("(10,49)", &union)]),
        (
            "short",
            vec![at(
                "(11,38)",
                "Argument of type 'number' is not assignable to parameter of type 'string'.",
            )],
        ),
        (
            "references",
            vec![
                at(
                    "(19,44)",
                    &format!(
                        "Argument of type '{{ a: \"{}\"; b: Wrapper<string>; c: number; }}' {ARG}",
                        "x".repeat(150)
                    ),
                ),
                at(
                    "(19,55)",
                    &format!(
                        "Argument of type '{{ a: \"{}\"; b: Wrapper<...>; c: number; }}' {ARG}",
                        "x".repeat(155)
                    ),
                ),
            ],
        ),
        (
            "index_counts",
            vec![
                at(
                    "(20,46)",
                    &format!(
                        "Argument of type '{{ t: {{ [k: string]: \"{}\"; }}; b: Wrapper<string>; }}' {ARG}",
                        "x".repeat(138)
                    ),
                ),
                at(
                    "(20,63)",
                    &format!(
                        "Argument of type '{{ t: {{ [k: string]: \"{}\"; }}; b: Wrapper<...>; }}' {ARG}",
                        "x".repeat(139)
                    ),
                ),
            ],
        ),
        (
            "labelled_tuple",
            vec![at(
                "(21,47)",
                &format!("Argument of type '[{}: string]' {ARG}", "a".repeat(90)),
            )],
        ),
        (
            "index_signature",
            vec![at(
                "(22,48)",
                &format!(
                    "Argument of type '{{ [k: string]: {{ {}; }}; }}' {ARG}",
                    properties[..20].join("; ")
                ),
            )],
        ),
        (
            "mapped_index",
            vec![at(
                "(24,45)",
                &format!(
                    "Argument of type '{{ [x: string]: {{ {}; }}; }}' {ARG}",
                    properties[..20].join("; ")
                ),
            )],
        ),
    ];
    let host = host_with(&source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (function, expected) in &rows {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host, root, function, &source,
                );
                if reported != *expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// A wide argument whose members are function queries, class-expression
/// instances or methods with type predicates prints as the checker
/// abbreviates it, never withheld: a `typeof` of a declared function prints
/// its signature, a class expression's instance `(Anonymous class)`, each
/// counted as the checker's node builder counts it, so the members past
/// the truncation length elide exactly where tsc elides them and a text of
/// twice that length is clipped. Every expected line is TypeScript 7.0.2's
/// on this fixture, identical with `strictNullChecks` off, cold and warm.
#[test]
fn wide_arguments_of_queried_and_class_expression_types_print_as_the_checker_abbreviates_them() {
    const WIDE: usize = 30;
    let mut source = String::from("\ndeclare function one(x: string): number;\n");
    for ordinal in 0..WIDE {
        source.push_str(&format!("declare function f{ordinal}(): void;\n"));
    }
    let members = |count: usize, member: &dyn Fn(usize) -> String| {
        (0..count).map(member).collect::<Vec<_>>().join("; ")
    };
    source.push_str(&format!(
        "declare namespace ns {{ function g(): void; }}\n\
         declare const typeofs: {{ {} }};\n\
         export function typeof_wide() {{ return one(typeofs); }}\n\
         declare const qualified: {{ {} }};\n\
         export function qualified_wide() {{ return one(qualified); }}\n\
         function mk() {{ return class {{ v = 1 }}; }}\n\
         const Made = mk();\n\
         declare const made: InstanceType<typeof Made>;\n\
         declare const anons: {{ {} }};\n\
         export function class_wide() {{ return one(anons); }}\n\
         export function class_short() {{ return one(made); }}\n\
         declare const preds: {{ {} }};\n\
         export function predicate_wide() {{ return one(preds); }}\n",
        members(WIDE, &|i| format!("p{i}: typeof f{i}")),
        members(WIDE, &|i| format!("q{i}: typeof ns.g")),
        members(WIDE, &|i| format!("a{i}: typeof made")),
        members(12, &|i| format!("r{i}(x: unknown): x is string")),
    ));
    let at = |position: &str, printed: &str| {
        format!(
            "{position}: error TS2345: Argument of type '{printed}' is not assignable to \
             parameter of type 'string'."
        )
    };
    let elided = |count: usize, member: &dyn Fn(usize) -> String| {
        format!(
            "{{ {}; ... {} more ...; {}; }}",
            members(count, member),
            WIDE - count - 1,
            member(WIDE - 1)
        )
    };
    let predicates = format!(
        "{{ {}; }}",
        members(12, &|i| format!("r{i}(x: unknown): x is string"))
    );
    let rows: Vec<(&str, Vec<String>)> = vec![
        (
            "typeof_wide",
            vec![at("(35,44)", &elided(16, &|i| format!("p{i}: () => void")))],
        ),
        (
            "qualified_wide",
            vec![at("(37,47)", &elided(16, &|i| format!("q{i}: () => void")))],
        ),
        (
            "class_wide",
            vec![at(
                "(42,43)",
                &elided(5, &|i| format!("a{i}: (Anonymous class)")),
            )],
        ),
        ("class_short", vec![at("(43,44)", "(Anonymous class)")]),
        (
            "predicate_wide",
            vec![at("(45,47)", &format!("{}...", &predicates[..317]))],
        ),
    ];
    let host = host_with(&source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (function, expected) in &rows {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host, root, function, &source,
                );
                if reported != *expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// A message prints a tuple and an object's members as the checker does:
/// under `strictNullChecks` an optional element's type with `undefined`
/// added, which also counts toward where the node builder abbreviates; an
/// object's call, construct and index signatures before its properties; and
/// an index signature by its authored parameter name, or `x` for one no
/// declaration wrote; and a mapped type that is not generic, at the top or
/// nested, as the object type it resolves to, `undefined` printed last; and
/// an optional `never` element or property as `undefined`.
/// Every expected line is TypeScript 7.0.2's on this fixture.
#[test]
fn messages_print_tuple_elements_and_object_members_as_the_checker_does() {
    let source = format!(
        "\ndeclare function one(x: string): number;\n\
         interface IX {{ [k: string]: number }}\n\
         interface Wrapper<T> {{ w: T }}\n\
         declare const t1: [lab: string, other?: number];\n\
         declare const t2: [number?, \"q\"?, (1 | 2)?];\n\
         declare const t3: {{ t: [lab?: \"{long}\"]; b: Wrapper<string>; c: number }};\n\
         declare const o1: {{ p: 1; [k: string]: \"q\" | 1 }};\n\
         declare const o2: {{ [k: string]: number; (): void; new (): void; p: 1 }};\n\
         declare const m1: {{ [P in keyof IX]: IX[P] }};\n\
         export function tuples() {{ return [one(t1), one(t2), one(t3)]; }}\n\
         export function members() {{ return [one(o1), one(o2), one(m1)]; }}\n\
         declare const m2: {{ [P in string]: 1 }};\n\
         declare const m3: {{ readonly [P in number]?: \"q\" }};\n\
         declare const m4: {{ a: {{ [P in \"a\" | \"b\"]?: 1 }} }};\n\
         export function mapped() {{ return [one(m2), one(m3), one(m4)]; }}\n\
         declare const n1: [lab?: never];\n\
         declare const n2: [never?];\n\
         declare const n3: {{ a?: never }};\n\
         export function nevers() {{ return [one(n1), one(n2), one(n3)]; }}\n",
        long = "x".repeat(140),
    );
    let at = |position: &str, printed: &str| {
        format!(
            "{position}: error TS2345: Argument of type '{printed}' is not assignable to parameter of type 'string'."
        )
    };
    let long = "x".repeat(140);
    let tuples = |strict: bool| {
        if strict {
            vec![
                at("(11,40)", "[lab: string, other?: number | undefined]"),
                at(
                    "(11,49)",
                    "[(number | undefined)?, (\"q\" | undefined)?, (1 | 2 | undefined)?]",
                ),
                at(
                    "(11,58)",
                    &format!(
                        "{{ t: [lab?: \"{long}\" | undefined]; b: Wrapper<...>; c: number; }}"
                    ),
                ),
            ]
        } else {
            vec![
                at("(11,40)", "[lab: string, other?: number]"),
                at("(11,49)", "[number?, \"q\"?, (1 | 2)?]"),
                at(
                    "(11,58)",
                    &format!("{{ t: [lab?: \"{long}\"]; b: Wrapper<string>; c: number; }}"),
                ),
            ]
        }
    };
    let members = vec![
        at("(12,41)", "{ [k: string]: \"q\" | 1; p: 1; }"),
        at(
            "(12,50)",
            "{ (): void; new (): void; [k: string]: number; p: 1; }",
        ),
        at("(12,59)", "{ [x: string]: number; }"),
    ];
    let mapped = |strict: bool| {
        vec![
            at("(16,40)", "{ [x: string]: 1; }"),
            at(
                "(16,49)",
                if strict {
                    "{ readonly [x: number]: \"q\" | undefined; }"
                } else {
                    "{ readonly [x: number]: \"q\"; }"
                },
            ),
            at(
                "(16,58)",
                if strict {
                    "{ a: { a?: 1 | undefined; b?: 1 | undefined; }; }"
                } else {
                    "{ a: { a?: 1; b?: 1; }; }"
                },
            ),
        ]
    };
    let nevers = |strict: bool| {
        if strict {
            vec![
                at("(20,40)", "[lab?: undefined]"),
                at("(20,49)", "[undefined?]"),
                at("(20,58)", "{ a?: undefined; }"),
            ]
        } else {
            vec![
                at("(20,40)", "[lab?: never]"),
                at("(20,49)", "[never?]"),
                at("(20,58)", "{ a?: never; }"),
            ]
        }
    };
    let host = host_with(&source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for (root, strict) in [(STRICT_ROOT, true), (LOOSE_ROOT, false)] {
            for (function, expected) in [
                ("tuples", tuples(strict)),
                ("members", members.clone()),
                ("mapped", mapped(strict)),
                ("nevers", nevers(strict)),
            ] {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host, root, function, &source,
                );
                if reported != expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// A failure through a long property path is reported while the checker's
/// relation stack holds it, and as TS2321 where the stack overflows: the
/// failing pair itself takes a frame when it is two object types or an
/// intersection target's member, not when it is two primitives or an
/// object type against a primitive. An overflow is reported twice, at the
/// call the checker compares the types in while it chooses the candidate,
/// then at the argument, in place of TS2345. Every line is TypeScript
/// 7.0.2's on this fixture, identical with `strictNullChecks` off.
#[test]
fn deep_chains_report_until_the_checker_relation_stack_overflows() {
    let nest = |inner: &str, depth: usize| {
        let mut ty = inner.to_owned();
        for _ in 0..depth {
            ty = format!("{{ a: {ty} }}");
        }
        ty
    };
    let rows: [(&str, usize, &str, &str); 8] = [
        ("object_leaf_99", 99, "{ x: number }", "{ y: number }"),
        ("object_leaf_100", 100, "{ x: number }", "{ y: number }"),
        ("object_to_primitive_99", 99, "{ x: 1 }", "string"),
        ("object_to_primitive_100", 100, "{ x: 1 }", "string"),
        ("primitive_leaf_100", 100, "string", "number"),
        ("primitive_leaf_101", 101, "string", "number"),
        ("intersection_98", 98, "string", "{ p: 1 } & { q: 2 }"),
        ("intersection_99", 99, "string", "{ p: 1 } & { q: 2 }"),
    ];
    let mut source = String::from("\n");
    for (name, depth, leaf_source, leaf_target) in rows {
        source.push_str(&format!(
            "declare function {name}_f(x: {}): void;\n\
             declare const {name}_s: {};\n\
             export function {name}() {{ return {name}_f({name}_s); }}\n",
            nest(leaf_target, depth),
            nest(leaf_source, depth),
        ));
    }
    let clipped = format!("{}{{ ...", "{ a: ".repeat(63));
    let reported = |position: &str, depth: usize, leaf: &str| {
        vec![format!(
            "{position}: error TS2345: Argument of type '{clipped}' is not assignable to parameter of type '{clipped}'.\n  \
             The types of '{}' are incompatible between these types.\n    {leaf}",
            vec!["a"; depth].join("."),
        )]
    };
    let overflowed = |call: &str, argument: &str| {
        [call, argument]
            .map(|position| {
                format!(
                    "{position}: error TS2321: Excessive stack depth comparing types \
                     '{clipped}' and '{clipped}'."
                )
            })
            .to_vec()
    };
    let expected: [(&str, Vec<String>); 8] = [
        (
            "object_leaf_99",
            reported(
                "(4,60)",
                99,
                "Property 'y' is missing in type '{ x: number; }' but required in type '{ y: number; }'.",
            ),
        ),
        ("object_leaf_100", overflowed("(7,44)", "(7,62)")),
        (
            "object_to_primitive_99",
            reported("(10,76)", 99, "Type '{ x: 1; }' is not assignable to type 'string'."),
        ),
        (
            "object_to_primitive_100",
            reported("(13,78)", 100, "Type '{ x: 1; }' is not assignable to type 'string'."),
        ),
        (
            "primitive_leaf_100",
            reported("(16,68)", 100, "Type 'string' is not assignable to type 'number'."),
        ),
        ("primitive_leaf_101", overflowed("(19,47)", "(19,68)")),
        (
            "intersection_98",
            reported(
                "(22,62)",
                98,
                "Type 'string' is not assignable to type '{ p: 1; } & { q: 2; }'.\n      \
                 Type 'string' is not assignable to type '{ p: 1; }'.",
            ),
        ),
        ("intersection_99", overflowed("(25,44)", "(25,62)")),
    ];
    let host = host_with(&source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (function, expected) in &expected {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host, root, function, &source,
                );
                if reported != *expected {
                    mismatches.push(format!(
                        "{pass} `{function}` in {root}: {reported:#?}, expected {expected:#?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

/// A diagnostic whose position the call's route cannot establish is not
/// reported, never moved to another position. TypeScript 7.0.2 reports
/// TS2345 for `un1`'s argument, which the route cannot type, while the
/// call still answers its lone signature's declared `number`.
#[test]
fn a_diagnostic_the_route_cannot_place_is_not_reported() {
    let host = host_with(CALL_RECOVERY);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        // The declared return stands, but the call's applicability went
        // unchecked: the answer is a recorded gap, never a complete one.
        super::signature_predicate_inference_tests::observe(
            &host,
            root,
            "un1",
            |dispatch, degradation, node| {
                assert!(
                    degradation.is_some(),
                    "`un1` in {root} answers its declared return as a recorded gap"
                );
                assert_eq!(
                    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                        dispatch, node, 0
                    ),
                    "number"
                );
            },
        );
        assert_eq!(
            super::signature_predicate_inference_tests::checker_report(
                &host,
                root,
                "un1",
                CALL_RECOVERY,
            ),
            Vec::<String>::new(),
            "`un1` in {root}"
        );
    }
}

const RECOVERY_AND_PREDICATE_REPORTS: &str = r#"type G<T> = G<[T]>;
declare function mk(x: number): G<string>;
export function circular() { return mk(1); }
export function circular_bad_argument() { return mk("x"); }
declare const guards: { isStr(x: string): x is "a" };
export function predicate(x: number | string) { if (guards.isStr(x)) return x; throw 0; }
"#;

/// A lazy declared return that is the checker's parameterized recovery
/// (TS2456, naming its alias) is reported with its bound argument at the
/// alias's authored name, and a lone predicate signature is checked for
/// applicability before it narrows. Measured on TypeScript 7.0.2.
#[test]
fn parameterized_recoveries_and_lone_predicates_report_like_the_checker() {
    const TS2456: &str = "(1,6): error TS2456: Type alias 'G' circularly references itself.";
    let host = host_with(RECOVERY_AND_PREDICATE_REPORTS);
    let checker_report = super::signature_predicate_inference_tests::checker_report;
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for pass in ["cold", "warm"] {
            let source = RECOVERY_AND_PREDICATE_REPORTS;
            assert_eq!(
                checker_report(&host, root, "circular", source),
                vec![TS2456],
                "{pass} `circular` in {root}"
            );
            assert_eq!(
                checker_report(&host, root, "circular_bad_argument", source),
                vec![
                    TS2456,
                    "(4,53): error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'."
                ],
                "{pass} `circular_bad_argument` in {root}"
            );
            assert_eq!(
                checker_report(&host, root, "predicate", source),
                vec![
                    "(6,66): error TS2345: Argument of type 'string | number' is not assignable to parameter of type 'string'.\n  Type 'number' is not assignable to type 'string'."
                ],
                "{pass} `predicate` in {root}"
            );
        }
    }
}

const LIB_VALUE_READS: &str = r#"
export function l1(x: number | number[]) { if (Array.isArray(x)) return x; throw 0; }
export function l2(x: number | number[]) { if (!Array.isArray(x)) return x; throw 0; }
export function l3(x: string | string[]) { return Array.isArray(x); }
export function f1(a: { readonly length: number; readonly [n: number]: number }) { return Array.from(a); }
export function f2() { return Array.of(1, 2); }
export function k1(o: { a: 1; b: 2 }) { return Object.keys(o); }
export function m1() { return Math.max(1, 2); }
export function m2() { return Math.PI; }
export function j1(s: string) { return JSON.parse(s); }
export function j2(v: unknown) { return JSON.stringify(v); }
export function n1(n: unknown) { return Number.isFinite(n); }
export function n2(x: unknown) { if (Number.isFinite(x)) return x; throw 0; }
export function sig_l3() { return l3; }
export function l4(x: any) { return Array.isArray(x); }
export function l5<T>(x: T | T[]) { return Array.isArray(x); }
export function sig_l4() { return l4; }
export function sig_l5() { return l5; }
"#;

/// A free `Array`, `Object`, `Math`, `JSON` or `Number` is the GLOBAL
/// VALUE the project's lib environment declares (`declare var Array:
/// ArrayConstructor;`), read through that declaration: its members'
/// signatures answer calls, and `Array.isArray`'s `arg is any[]` narrows
/// both edges and is the predicate a returned call infers, over `any`
/// (`x is any[]`) and a type parameter (`x is T[]`) too. Measured on
/// 7.0.2 against the full lib, identically without `strictNullChecks`;
/// the host registers the lib's own declarations of these values.
#[test]
fn a_lib_global_value_reads_its_lib_declaration() {
    let rows = [
        ("l1", "number[]"),
        ("l2", "number"),
        ("f1", "number[]"),
        ("f2", "number[]"),
        ("k1", "string[]"),
        ("m1", "number"),
        ("m2", "number"),
        ("j1", "any"),
        ("j2", "string"),
        ("n1", "boolean"),
        ("n2", "unknown"),
    ];
    let host = super::signature_predicate_inference_tests::host_with_lib(LIB_VALUE_READS);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, printed) in rows {
            assert_prints(&host, root, function, printed);
        }
        let predicate = "(x: string | string[]) => x is string[]";
        assert_prints(&host, root, "sig_l3", predicate);
        assert_predicate(&host, root, "sig_l3", predicate);
        let predicate = "(x: any) => x is any[]";
        assert_prints(&host, root, "sig_l4", predicate);
        assert_predicate(&host, root, "sig_l4", predicate);
        assert_renders(
            &host,
            root,
            "sig_l5",
            "{ (Union(TypeParam(T) | Array(TypeParam(T)))) => x is Array(TypeParam(T)) }",
        );
    }
}

/// Without a lib environment a free global value names no declaration:
/// the call through it stays the typed gap, never a guessed signature.
#[test]
fn a_lib_global_value_without_a_lib_stays_the_typed_gap() {
    let host = host_with(LIB_VALUE_READS);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for function in ["l1", "m1", "sig_l3"] {
            super::signature_predicate_inference_tests::assert_degrades(&host, root, function);
        }
    }
}

const ASSERTED_WRITES: &str = r#"
export class Base { b = 1 }
export function w01(x: string | number) { (x as any) = 1; return x; }
export function w02(x: number | string) { (x as any)++; return x; }
export function w03(x: string | number) { (<any>x) = 1; return x; }
export function w04(x: string | number) { (x satisfies any) = 1; return x; }
export function w05(x: string | number) { [(x as any)] = [1]; return x; }
export function w06(x: string | number) { ({ a: (x as any) } = { a: 1 }); return x; }
export function w07(x: string | number) { x! = 1; return x; }
export function w08(x: string | number) { (x) = 1; return x; }
export function w10(x: string | number | undefined) { (x as any) ??= 1; return x; }
export function w11(x: string | number) { for ((x as any) of [1]) { } return x; }
export function w12() { let x: string | number = "a"; (x as any) = 1; return x; }
export function w14() { let x = "a"; (x as any) = 1; return x; }
export function w15(x: number | string) { (x as number) += 1; return x; }
export function w16(x: string | number) { ((x as any)) = 1; return x; }
export function w17(x: string | number) { (x! as any) = 1; return x; }
export function w18(x: string | number) { ((x as any)!) = 1; return x; }
export function w19(x: string | number) { if (typeof x === "string") { (x as any) = 1; return x; } throw 0; }
export function s2(x: string | number) { if (typeof x === "string") { class C { static s = (x = 1); } return x; } throw 0; }
export function s6(x: string | number) { if (typeof x === "string") { class C { v = (x = 1); } return x; } throw 0; }
"#;

/// A write through a type assertion (`(x as T) = v`, `(<T>x) = v`,
/// `(x satisfies T) = v`, a destructuring element, an update, a compound
/// or logical assignment, a loop head) neither assigns nor narrows the
/// binding: the checker's narrowable reference and assignment target both
/// stop at an assertion, so the read after it keeps its type. A non-null
/// assertion or parentheses around the target still assign (`x! = 1` and
/// `(x) = 1` read `number`). A class's property initializer is its own
/// control-flow container, so its write does not retype the enclosing
/// read. Measured on 7.0.2, identically without `strictNullChecks` except
/// `w10`, whose `undefined` the loose parameter type never had.
#[test]
fn a_write_through_a_type_assertion_neither_assigns_nor_narrows() {
    let rows = [
        ("w01", "string | number", "string | number"),
        ("w02", "string | number", "string | number"),
        ("w03", "string | number", "string | number"),
        ("w04", "string | number", "string | number"),
        ("w05", "string | number", "string | number"),
        ("w06", "string | number", "string | number"),
        ("w07", "number", "number"),
        ("w08", "number", "number"),
        ("w10", "string | number | undefined", "string | number"),
        ("w11", "string | number", "string | number"),
        ("w12", "string", "string"),
        ("w14", "string", "string"),
        ("w15", "string | number", "string | number"),
        ("w16", "string | number", "string | number"),
        ("w17", "string | number", "string | number"),
        ("w18", "string | number", "string | number"),
        ("w19", "string", "string"),
        ("s2", "string", "string"),
        ("s6", "string", "string"),
    ];
    check_rows(ASSERTED_WRITES, &rows);
}

/// `symbol`'s answer in `root` is COMPLETE and renders as `rendered` — the
/// pin for a checker answer over a type parameter, which the checker print
/// grammar spells as a name.
fn assert_renders(host: &crate::VerterHost, root: &str, symbol: &str, rendered: &str) {
    super::signature_predicate_inference_tests::observe(
        host,
        root,
        symbol,
        |dispatch, degradation, node| {
            let measured = crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                dispatch, node, 0,
            );
            assert!(
                degradation.is_none() && measured == rendered,
                "`{symbol}` in {root} renders `{rendered}`; measured `{measured}` (degradation \
                 {degradation:?})"
            );
        },
    );
}

const TOP_TYPE_GUARDS: &str = r#"
export class Cls { c = 1 }
export interface Foo { kind: "foo"; n: number }
declare function isFoo(x: unknown): x is Foo;
declare function isFn(x: unknown): x is Function;
export function y01(x: any) { return isFoo(x); }
export function y02(x: any) { return x instanceof Cls; }
export function y03(x: any) { return "a" in x; }
export function y11(x: any) { if (isFoo(x)) { return x; } throw 0; }
export function y12(x: any) { if (!isFoo(x)) { return x; } throw 0; }
export function y13(x: any) { if (x instanceof Cls) { return x; } throw 0; }
export function y14(x: any) { if (!(x instanceof Cls)) { return x; } throw 0; }
export function y15(x: any) { if ("a" in x) { return x; } throw 0; }
export function y16(x: unknown) { if (x instanceof Cls) { return x; } throw 0; }
export function y17(x: unknown) { if (!(x instanceof Cls)) { return x; } throw 0; }
export function y19(x: any) { if (isFn(x)) { return x; } throw 0; }
export function y21(x: unknown) { return x instanceof Cls; }
"#;

/// `any` narrows to a type predicate's target and to an `instanceof`
/// test's instance type on the true edge and keeps itself on the false
/// one (`getNarrowedType` answers the candidate for a top type), except
/// that the checker never narrows it to the global `Function`
/// (`narrowTypeByTypePredicate`); `unknown` narrows the same way under
/// `instanceof`; an `in` test leaves `any` as it is. Measured on 7.0.2,
/// identically without `strictNullChecks`, each function's `.d.ts` line
/// the row's answer.
#[test]
fn a_top_type_narrows_to_a_predicate_or_instance_target() {
    let rows = [
        ("y11", "Foo", "Foo"),
        ("y12", "any", "any"),
        ("y13", "Cls", "Cls"),
        ("y14", "any", "any"),
        ("y15", "any", "any"),
        ("y16", "Cls", "Cls"),
        ("y17", "unknown", "unknown"),
        ("y19", "any", "any"),
    ];
    check_rows(TOP_TYPE_GUARDS, &rows);
    let predicates = [
        ("y01", "(x: any) => x is Foo"),
        ("y02", "(x: any) => x is Cls"),
        ("y03", "(x: any) => boolean"),
        ("y21", "(x: unknown) => x is Cls"),
    ];
    let mut source = String::from(TOP_TYPE_GUARDS);
    for (name, _) in &predicates {
        source.push_str(&format!(
            "export function sig_{name}() {{ return {name}; }}\n"
        ));
    }
    let host = host_with(&source);
    for (name, printed) in predicates {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            assert_prints(&host, root, &format!("sig_{name}"), printed);
            assert_predicate(&host, root, &format!("sig_{name}"), printed);
        }
    }
}

const GENERIC_GUARDS: &str = r#"
export interface Foo { kind: "foo"; n: number }
declare function isFoo(x: unknown): x is Foo;
declare function isArr(x: any): x is any[];
declare function isStr(x: unknown): x is string;
export function g01<T>(x: T | Foo) { return isFoo(x); }
export function g01v<T>(x: T | Foo) { if (isFoo(x)) { return x; } throw 0; }
export function g01e<T>(x: T | Foo) { if (!isFoo(x)) { return x; } throw 0; }
export function g02<T>(x: T | T[]) { if (isArr(x)) { return x; } throw 0; }
export function g03<T>(x: T | T[]) { if (!isArr(x)) { return x; } throw 0; }
export function g02p<T>(x: T | T[]) { return isArr(x); }
export function g04<T>(x: T) { return isArr(x); }
export function g05<T>(x: T) { if (isArr(x)) { return x; } throw 0; }
export function g05e<T>(x: T) { if (!isArr(x)) { return x; } throw 0; }
export function g06<T extends string | number[]>(x: T) { return isArr(x); }
export function g07<T extends string | number[]>(x: T) { if (isArr(x)) { return x; } throw 0; }
export function g07e<T extends string | number[]>(x: T) { if (!isArr(x)) { return x; } throw 0; }
export function g08<T extends object>(x: T) { if (isFoo(x)) { return x; } throw 0; }
export function g09<T extends Foo>(x: T) { return isFoo(x); }
export function g09v<T extends Foo>(x: T) { if (isFoo(x)) { return x; } throw 0; }
export function g11<T extends string | number>(x: T) { if (isStr(x)) { return x; } throw 0; }
export function g11p<T extends string | number>(x: T) { return isStr(x); }
export function g12<T>(x: T | string) { if (isStr(x)) { return x; } throw 0; }
export function g12e<T>(x: T | string) { if (!isStr(x)) { return x; } throw 0; }
export function g13<T extends string>(x: T | number) { if (isStr(x)) { return x; } throw 0; }
export function g14<T>(x: T | undefined) { if (isFoo(x)) { return x; } throw 0; }
export function c15<T>(x: T, g: (v: unknown) => v is T[]) { if (g(x)) return x; throw 0; }
"#;

/// A predicate narrows a type parameter through its constraint
/// (`getNarrowedType`'s instantiable reading): an arm assignable to the
/// target survives (`T[]` under `x is any[]`; a `T extends Foo` under `x
/// is Foo`, unchanged there, so no predicate is inferred); failing that
/// the target itself when another arm admits it (`string` from `T |
/// string`); failing that each type-parameter arm whose constraint admits
/// the target becomes their intersection (`T & any[]`, `T & Foo` beside a
/// dropped `undefined`); and the false edge keeps what is not assignable
/// to the target. Measured on 7.0.2, identically without
/// `strictNullChecks` (each function's `.d.ts` line): `g01v` `Foo`, `g01e`
/// `T`, `g02` `T[]`, `g03` `T`, `g05` and `g07` `T & any[]`, `g05e` and
/// `g07e` `T`, `g08` `T & Foo`, `g09v` `T`, `g11` `T & string`, `g12`
/// `string`, `g12e` `T`, `g13` `T`, `g14` `T & Foo`, `c15` `T & T[]`; the
/// predicates `x is Foo` (`g01`), `x is T[]` (`g02p`), `x is T & any[]`
/// (`g04`, `g06`), `x is T & string` (`g11p`) and none for `g09`. The
/// print grammar spells a type parameter as a name, so the rows pin the
/// rendered node.
#[test]
fn a_type_parameter_narrows_through_its_constraint() {
    let rows = [
        ("g01v", "DeclRef(Foo)"),
        ("g01e", "TypeParam(T)"),
        ("g02", "Array(TypeParam(T))"),
        ("g03", "TypeParam(T)"),
        ("g05", "Intersection(TypeParam(T) & Array(any))"),
        ("g05e", "TypeParam(T)"),
        ("g07", "Intersection(TypeParam(T) & Array(any))"),
        ("g07e", "TypeParam(T)"),
        ("g08", "Intersection(TypeParam(T) & DeclRef(Foo))"),
        ("g09v", "TypeParam(T)"),
        ("g11", "Intersection(TypeParam(T) & string)"),
        ("g12", "string"),
        ("g12e", "TypeParam(T)"),
        ("g13", "TypeParam(T)"),
        ("g14", "Intersection(TypeParam(T) & DeclRef(Foo))"),
        ("c15", "Intersection(TypeParam(T) & Array(TypeParam(T)))"),
    ];
    let predicates = [
        (
            "g01",
            "{ (Union(TypeParam(T) | DeclRef(Foo))) => x is DeclRef(Foo) }",
        ),
        (
            "g02p",
            "{ (Union(TypeParam(T) | Array(TypeParam(T)))) => x is Array(TypeParam(T)) }",
        ),
        (
            "g04",
            "{ (TypeParam(T)) => x is Intersection(TypeParam(T) & Array(any)) }",
        ),
        (
            "g06",
            "{ (TypeParam(T)) => x is Intersection(TypeParam(T) & Array(any)) }",
        ),
        ("g09", "{ (TypeParam(T)) => boolean }"),
        (
            "g11p",
            "{ (TypeParam(T)) => x is Intersection(TypeParam(T) & string) }",
        ),
    ];
    let mut source = String::from(GENERIC_GUARDS);
    for (name, _) in &predicates {
        source.push_str(&format!(
            "export function sig_{name}() {{ return {name}; }}\n"
        ));
    }
    let host = host_with(&source);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, rendered) in rows {
            assert_renders(&host, root, function, rendered);
        }
        for (function, rendered) in predicates {
            assert_renders(&host, root, &format!("sig_{function}"), rendered);
        }
    }
}

const LOGICAL_VALUES: &str = r#"
export function l1(x: boolean) { return x && "a"; }
export function l2(x: string) { return x && 1; }
export function l3(x: string | undefined) { return x || "d"; }
export function l4(x: number) { return x || "z"; }
export function l5(x: boolean) { const v = x && "a"; return v; }
export function l6(x: boolean) { return { v: x && "a" }; }
export function l8(x: null) { return x && 1; }
export function l9(x: { a: 1 }) { return x || 2; }
export function l10(x: boolean, y: boolean) { return x && y; }
export function l11(x: 0 | 1) { return x && "a"; }
export function l12(x: string | number) { return typeof x === "string" && x !== ""; }
export function l13(x: any) { return x && 1; }
export function l14(x: unknown) { return x || 1; }
export function l15(x: string | undefined) { let v = x || "d"; return v; }
export function l17(x: {} | null) { return x || 1; }
export function l18(x: string | number) { return typeof x === "string" || x > 0; }
"#;

/// A `&&` / `||` in value position is typed as the checker types it
/// (`checkBinaryLikeExpression`): the right operand reads under the left's
/// truthy (`&&`) or falsy (`||`) narrowing; `&&` joins the left's
/// definitely-falsy part — of the widened right without
/// `strictNullChecks` — with the right, or is the left alone when it
/// cannot be truthy; `||` joins the left's possibly-truthy part
/// (`boolean` as `true`, `unknown` as `{}`, no `undefined`) with the
/// right under subtype reduction, or is the left alone when it cannot be
/// falsy. A mutable slot widens the operands' fresh literals, never the
/// falsy part. Measured on 7.0.2 (the `.d.ts` line, strict and without
/// `strictNullChecks`).
#[test]
fn a_logical_expression_value_is_typed_like_the_checker() {
    let rows = [
        ("l1", "\"a\" | false", "\"\" | \"a\""),
        ("l2", "\"\" | 1", "0 | 1"),
        ("l3", "string", "string"),
        ("l4", "number | \"z\"", "number | \"z\""),
        ("l5", "\"a\" | false", "\"\" | \"a\""),
        ("l6", "{ v: string | false; }", "{ v: string; }"),
        ("l8", "null", "null"),
        ("l9", "{ a: 1; }", "2 | { a: 1; }"),
        ("l10", "boolean", "boolean"),
        ("l11", "\"a\" | 0", "\"\" | \"a\""),
        ("l12", "boolean", "boolean"),
        ("l13", "any", "0 | 1"),
        ("l14", "{}", "unknown"),
        ("l15", "string", "string"),
        ("l17", "{}", "{}"),
        ("l18", "boolean", "boolean"),
    ];
    check_rows(LOGICAL_VALUES, &rows);
}

/// The lib environment the logical-predicate rows register: the pinned
/// lib's own declarations of the globals they read (`lib.es5.d.ts` and
/// `lib.es2015.core.d.ts`), each narrowed to the members read.
const LOGICAL_LIB: &str = r#"
interface Array<T> { length: number; }
interface ArrayConstructor { isArray(arg: any): arg is any[]; }
declare var Array: ArrayConstructor;
interface NumberConstructor { isFinite(number: unknown): boolean; }
declare var Number: NumberConstructor;
declare function isNaN(number: number): boolean;
interface Math { abs(x: number): number; }
declare var Math: Math;
interface JSON { parse(text: string, reviver?: (this: any, key: string, value: any) => any): any; }
declare var JSON: JSON;
"#;

const LOGICAL_PREDICATES: &str = r#"
export function a07(x: string | number | boolean[]) { return typeof x === "string" || Array.isArray(x); }
export function a10(x: unknown) { return typeof x === "number" && Number.isFinite(x); }
export function a11(x: string | number) { return typeof x === "number" && !isNaN(x); }
export function a12(x: string | number[]) { return Array.isArray(x) && x.length > 0; }
export function a14(x: string | number) { return typeof x === "string" && JSON.parse(x) !== null; }
export function a15(x: string | number) { return typeof x === "number" && Math.abs(x) > 1; }
"#;

/// A returned `&&` / `||` that calls a lib global (`Array.isArray`,
/// `Number.isFinite`, the global `isNaN`, `Math.abs`, `JSON.parse`) is a
/// `boolean` value, so the checker infers the predicate its test
/// establishes, or none: only `Array.isArray`'s `arg is any[]` narrows,
/// and a call inside a comparison is off the narrowing spine. The lib's
/// own declaration is a global's only one when no program file declares
/// it. Measured on 7.0.2, identically without `strictNullChecks`: `a07`
/// is `(x: string | number | boolean[]) => x is string | boolean[]`, the
/// rest return `boolean` with no predicate.
#[test]
fn a_returned_logical_test_over_lib_calls_infers_like_the_checker() {
    let rows = [
        (
            "a07",
            "(x: string | number | boolean[]) => x is string | boolean[]",
        ),
        ("a10", "(x: unknown) => boolean"),
        ("a11", "(x: string | number) => boolean"),
        ("a12", "(x: string | number[]) => boolean"),
        ("a14", "(x: string | number) => boolean"),
        ("a15", "(x: string | number) => boolean"),
    ];
    let mut source = String::from(LOGICAL_PREDICATES);
    for (name, _) in &rows {
        source.push_str(&format!(
            "export function sig_{name}() {{ return {name}; }}\n"
        ));
    }
    let host =
        super::signature_predicate_inference_tests::host_with_lib_source(&source, LOGICAL_LIB);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (name, printed) in rows {
            super::signature_predicate_inference_tests::assert_complete(&host, root, name);
            assert_prints(&host, root, &format!("sig_{name}"), printed);
            assert_predicate(&host, root, &format!("sig_{name}"), printed);
        }
    }
}

const ENTERED_CALLS: &str = r#"
declare function assertString(x: unknown): asserts x is string;
declare function assertTruthy(x: unknown): asserts x;
declare function g(v: unknown): void;
function fail(): never { throw 0; }
export function n1(x: string | number) { g(assertString(x)); return x; }
export function n2(x: string | number) { const y = assertString(x); return x; }
export function n3(x: string | number) { const y = [assertString(x)]; return { y, x }; }
export function n4(x: string | number) { void assertString(x); return x; }
export function n5(x: string | number) { `${assertString(x)}`; return x; }
export function n6(x: string | number) { const y = (0, assertString(x)); return { y, x }; }
export function n7(x: string | number) { g((0, assertString(x))); return x; }
export function p1(x: string | number) { const y = (0, assertString(x)); return x; }
export function p2(x: string | number) { const y = (0, (assertString(x))); return x; }
export function p3(x: string | number) { (assertString(x)); return x; }
export function p4(x: string | number) { const y = ((assertString(x)), 0); return x; }
export function p5(x: string | number) { void (0, assertString(x)); return x; }
export function p6(x: string | number) { const y = (0, 1, assertString(x)); return x; }
export function p7(x: string | number) { const y = (assertString(x), 1, 2); return x; }
export function p8(x: string | number) { let y; y = (0, assertString(x)); return x; }
export function p9(x: string | number) { const y = [(0, assertString(x))]; return x; }
export function p11(x: string | undefined) { const y = (0, assertTruthy(x)); return x; }
export function p12(x: string | number) { return ((0, assertString(x)), x); }
export function p15(x: string | number, c: boolean) { const y = c ? (0, assertString(x)) : 0; return x; }
export function p17(x: string | number) { if ((0, assertString(x))) { } return x; }
export function p19(x: string | number) { (assertString(x), 0); return x; }
export function p22(x: string | number) { const y = (assertString(x), x); return y; }
export function j1(x: string | number, c: boolean) { const y = c ? 0 : (0, assertString(x)); return x; }
export function j2(x: string | number, c: boolean) { const y = c && (0, assertString(x)); return x; }
export function j3(x: string | number) { const y = true ? (0, assertString(x)) : 0; return x; }
export function j5(x: string | number, c: boolean) { const y = ((0, assertString(x)) ? 1 : 2); return { y, x }; }
export function k7(x: string | number) { true && (assertString(x), true); return x; }
export function f8(x: string | number) { if (typeof x === "number") { (0, fail()); } return x; }
"#;

/// The checker enters a call into control flow — where an `asserts`
/// callee narrows what follows — only when it is an UNPARENTHESIZED
/// expression statement's own call or comma operand
/// (`maybeBindExpressionFlowIfCall`), and the narrowing persists past the
/// position unless a conditional's other path joins it unnarrowed.
/// Measured on 7.0.2, identically without `strictNullChecks`: an argument
/// (`n1`), an initializer (`n2`), an array element (`n3`), a `void`
/// operand (`n4`), a template interpolation (`n5`) and a parenthesized
/// call (`p2`, `p3`, `p4`) leave `x` as `string | number`; a comma
/// operand narrows `x` to `string` wherever the sequence sits — an
/// initializer, an argument, an array element, an assignment, a `void`
/// operand, an `if` or ternary test, a statement (`n6`, `n7`, `p1`, `p5`
/// to `p9`, `p11`, `p12`, `p17`, `p19`, `p22`, `j5`); inside one arm of a
/// conditional it narrows nothing past it (`p15`, `j1`, `j2`), unless a
/// literal test makes that arm certain (`j3`, `k7`). A never-returning
/// comma operand ends the path like its statement twin (`f8` is `string`).
#[test]
fn only_an_entered_call_can_assert() {
    let rows = [
        ("n1", "string | number", "string | number"),
        ("n2", "string | number", "string | number"),
        (
            "n3",
            "{ y: void[]; x: string | number; }",
            "{ y: void[]; x: string | number; }",
        ),
        ("n4", "string | number", "string | number"),
        ("n5", "string | number", "string | number"),
        ("n6", "{ y: void; x: string; }", "{ y: void; x: string; }"),
        ("n7", "string", "string"),
        ("p1", "string", "string"),
        ("p2", "string | number", "string | number"),
        ("p3", "string | number", "string | number"),
        ("p4", "string | number", "string | number"),
        ("p5", "string", "string"),
        ("p6", "string", "string"),
        ("p7", "string", "string"),
        ("p8", "string", "string"),
        ("p9", "string", "string"),
        ("p11", "string", "string"),
        ("p12", "string", "string"),
        ("p15", "string | number", "string | number"),
        ("p17", "string", "string"),
        ("p19", "string", "string"),
        ("p22", "string", "string"),
        ("j1", "string | number", "string | number"),
        ("j2", "string | number", "string | number"),
        ("j3", "string", "string"),
        (
            "j5",
            "{ y: number; x: string; }",
            "{ y: number; x: string; }",
        ),
        ("k7", "string", "string"),
        ("f8", "string", "string"),
    ];
    check_rows(ENTERED_CALLS, &rows);
}

/// A comma operand in the argument of a statement call whose callee is an
/// unresolved import narrows like any other: measured on 7.0.2, with and
/// without `strictNullChecks` (and with the import resolved to
/// `declare function touch(v: unknown): void`), `n8` is `string`. The
/// lane reads `string` behind the typed guard gap, because the callee's
/// effects signature is not settled.
#[test]
#[ignore = "a statement call whose callee is an unresolved import settles no effects signature, \
            so the argument's entered assertion publishes behind the typed guard gap"]
fn an_unresolved_import_statement_callee_keeps_its_arguments_assertion() {
    const SOURCE: &str = r#"
import { touch } from "./touch";
declare function assertString(x: unknown): asserts x is string;
export function n8(x: string | number) { touch((assertString(x), 0)); return x; }
"#;
    check_rows(SOURCE, &[("n8", "string", "string")]);
}

const EMPTY_OBJECT_TARGETS: &str = r#"
export function e1() { const v: 1 extends {} ? 1 : 0 = null as any; return v; }
export function e2() { const v: "a" extends {} ? 1 : 0 = null as any; return v; }
export function e3() { const v: number extends {} ? 1 : 0 = null as any; return v; }
export function e4() { const v: null extends {} ? 1 : 0 = null as any; return v; }
export function e5() { const v: number[] extends {} ? 1 : 0 = null as any; return v; }
export function e6() { const v: [1] extends {} ? 1 : 0 = null as any; return v; }
export function e7() { const v: (() => void) extends {} ? 1 : 0 = null as any; return v; }
export function e8() { const v: undefined extends {} ? 1 : 0 = null as any; return v; }
export function z5(x: unknown) { return x === null || Array.isArray(x); }
export function z6(x: unknown) { if (x === null || Array.isArray(x)) { return x; } throw 0; }
export function sig_z5() { return z5; }
"#;

/// Every value but `null`, `undefined` and `void` is assignable to the
/// empty object type `{}` — a primitive or literal through its empty
/// apparent surface, an array, a tuple or a function because it is an
/// object — so `unknown` narrowed past `null` (`{} | undefined`) narrows
/// to `any[]` under `Array.isArray`, the `{}` arm taking the narrower
/// target (`getNarrowedType`). Measured on 7.0.2: `e1`–`e3` and `e5`–`e7`
/// are `1`; `e4` and `e8` are `0` (`1` without `strictNullChecks`); `z5`
/// is `(x: unknown) => x is any[] | null` and `z6` `any[] | null`
/// (`boolean` and `unknown` without `strictNullChecks`, where `x === null`
/// narrows nothing).
#[test]
fn every_non_nullish_value_is_assignable_to_the_empty_object_type() {
    let rows = [
        ("e1", "1", "1"),
        ("e2", "1", "1"),
        ("e3", "1", "1"),
        ("e4", "0", "1"),
        ("e5", "1", "1"),
        ("e6", "1", "1"),
        ("e7", "1", "1"),
        ("e8", "0", "1"),
        ("z6", "any[] | null", "unknown"),
    ];
    let host = super::signature_predicate_inference_tests::host_with_lib(EMPTY_OBJECT_TARGETS);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, strict, loose) in rows {
            let printed = if root == STRICT_ROOT { strict } else { loose };
            assert_prints(&host, root, function, printed);
        }
    }
    for (root, printed) in [
        (STRICT_ROOT, "(x: unknown) => x is any[] | null"),
        (LOOSE_ROOT, "(x: unknown) => boolean"),
    ] {
        assert_prints(&host, root, "sig_z5", printed);
        assert_predicate(&host, root, "sig_z5", printed);
    }
}

const OFF_SPINE_CALLS: &str = r#"
function touch() {}
export function s1(x: string | number) { if (typeof x === "number" && Math.abs(x) > 1) { return x; } throw 0; }
export function s2(x: string | number) { if (typeof x === "string" && JSON.parse(x) !== null) { return x; } throw 0; }
export function s3(x: "a" | "b") { if ((touch(), x) === "a") { return x; } throw 0; }
"#;

/// A call inside a comparison operand of an `if` test is off the narrowing
/// spine: its predicate narrows nothing, and it is never entered into
/// control flow, so the test narrows exactly as its spine does. Measured
/// on 7.0.2, identically without `strictNullChecks`: `s1` is `number`,
/// `s2` `string`. A comparison whose reference sits behind a comma
/// sequence narrows that reference (`s3` is `"a"`).
#[test]
fn a_call_off_the_narrowing_spine_narrows_nothing() {
    let host = super::signature_predicate_inference_tests::host_with_lib_source(
        OFF_SPINE_CALLS,
        LOGICAL_LIB,
    );
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        assert_prints(&host, root, "s1", "number");
        assert_prints(&host, root, "s2", "string");
        assert_prints(&host, root, "s3", "\"a\"");
    }
}

const COMMA_REFERENCES: &str = r#"
export interface Foo { kind: "foo"; n: number }
export interface Bar { kind: "bar"; s: string }
export class Cls { c = 1 }
function touch() { }
function isFoo(x: unknown): x is Foo { return true; }
export function c1(x: "a" | "b") { if ((touch(), x) === "a") { return x; } throw 0; }
export function c2(x: string | number) { if (typeof (touch(), x) === "string") { return x; } throw 0; }
export function c3(x: string | undefined) { if ((touch(), x)) { return x; } throw 0; }
export function c4(x: unknown) { if (isFoo((touch(), x))) { return x; } throw 0; }
export function c5(x: Cls | string) { if ((touch(), x) instanceof Cls) { return x; } throw 0; }
export function c6(x: Foo | Bar) { if ("n" in (touch(), x)) { return x; } throw 0; }
export function c7(u: Foo | Bar) { return { v: (touch(), u.kind) === "foo" ? u.n : 0 }; }
export function c8(x: "a" | "b") { return (touch(), x) === "a"; }
export function c9(x: string | number) { if (typeof ((touch(), x)) === "string") { return x; } throw 0; }
export function c10(x: string | number, y: string | number) { if (typeof (y, x) === "string") { return x; } throw 0; }
export function c11(x: "a" | "b") { if ((0, 1, x) !== "a") { return x; } throw 0; }
export function c12(x: string | null) { if ((touch(), x) != null) { return x; } throw 0; }
export function sig_c8() { return c8; }
"#;

/// A narrow lands on the reference a comma sequence's LAST operand is
/// (the checker's `getReferenceCandidate` and `isMatchingReference`): an
/// equality, a `typeof`, a truthiness test, a predicate argument, an
/// `instanceof` or `in` operand and a discriminant read behind `(touch(),
/// x)` all narrow `x`, the earlier operands only running first. Measured
/// on 7.0.2, identically without `strictNullChecks`.
#[test]
fn a_reference_behind_a_comma_sequence_narrows() {
    let rows = [
        ("c1", "\"a\"", "\"a\""),
        ("c2", "string", "string"),
        ("c3", "string", "string"),
        ("c4", "Foo", "Foo"),
        ("c5", "Cls", "Cls"),
        ("c6", "Foo", "Foo"),
        ("c7", "{ v: number; }", "{ v: number; }"),
        ("c9", "string", "string"),
        ("c10", "string", "string"),
        ("c11", "\"b\"", "\"b\""),
        ("c12", "string", "string"),
    ];
    check_rows(COMMA_REFERENCES, &rows);
    check_predicate_rows(
        COMMA_REFERENCES,
        &[(
            "sig_c8",
            "(x: \"a\" | \"b\") => x is \"a\"",
            "(x: \"a\" | \"b\") => x is \"a\"",
        )],
    );
}

const TYPEOF_EMPTY_OBJECT: &str = r#"
export function t1(x: unknown) { if (typeof x === "object") return x; throw 0; }
export function t2(x: {}) { if (typeof x === "object") return x; throw 0; }
export function t3(x: {} | undefined) { if (typeof x === "object") return x; throw 0; }
export function t4(x: {}) { if (typeof x === "function") return x; throw 0; }
export function t5(x: {}) { if (typeof x !== "object") return x; throw 0; }
export function t6(x: unknown) { return x === null || typeof x === "object"; }
export function t7(x: unknown) { return typeof x === "object"; }
export function t8(x: {} | null) { return typeof x === "object"; }
export function t9(x: unknown) { if (x !== null && typeof x === "object") return x; throw 0; }
export function t10(x: {}) { return typeof x === "object"; }
export function t11(x: unknown) { if (x == null || typeof x === "object") return x; throw 0; }
export function t12(x: {}) { if (typeof x === "string") return x; throw 0; }
export function u1(x: {}) { if (typeof x === "undefined") return x; throw 0; }
export function u2(x: {}) { if (typeof x !== "undefined") return x; throw 0; }
export function u4(x: {}) { if (typeof x !== "string") return x; throw 0; }
export function u5(x: {}) { if (typeof x === "number") return x; throw 0; }
export function u7(x: {}) { if (typeof x === "symbol") return x; throw 0; }
export function u8(x: {}) { if (typeof x === "bigint") return x; throw 0; }
export function u9(x: {} | string) { if (typeof x === "object") return x; throw 0; }
export function u10(x: {} | null) { if (typeof x === "object") return x; throw 0; }
export function u11(x: {} | undefined) { if (typeof x === "undefined") return x; throw 0; }
export function u12(x: {} | undefined) { if (typeof x !== "undefined") return x; throw 0; }
export function sig_t6() { return t6; }
export function sig_t7() { return t7; }
export function sig_t8() { return t8; }
export function sig_t10() { return t10; }
"#;

/// The empty object type `{}` holds every value but `null` and
/// `undefined`, so a `typeof` test narrows it to the kind's implied type
/// without them (`narrowTypeByTypeof`): `object` under `"object"`, the
/// global `Function` under `"function"`, `string`, `number`, `symbol`,
/// `bigint` under theirs, nothing under `"undefined"` with
/// `strictNullChecks` (`undefined` without it); the negated edge keeps
/// `{}`. With `unknown` split into `{} | null | undefined` past a nullish
/// test, a returned `x === null || typeof x === "object"` infers `x is
/// object | null`. Measured on 7.0.2 (the `.d.ts` line, strict and without
/// `strictNullChecks`).
#[test]
fn a_typeof_test_narrows_the_empty_object_type() {
    let rows = [
        ("t1", "object | null", "object"),
        ("t2", "object", "object"),
        ("t3", "object", "object"),
        ("t5", "{}", "{}"),
        ("t9", "object", "object"),
        ("t11", "object | null | undefined", "unknown"),
        ("t12", "string", "string"),
        ("u1", "never", "undefined"),
        ("u2", "{}", "{}"),
        ("u4", "{}", "{}"),
        ("u5", "number", "number"),
        ("u7", "symbol", "symbol"),
        ("u8", "bigint", "bigint"),
        ("u9", "object", "object"),
        ("u10", "object | null", "object"),
        ("u11", "undefined", "undefined"),
        ("u12", "{}", "{}"),
    ];
    check_rows(TYPEOF_EMPTY_OBJECT, &rows);
    check_predicate_rows(
        TYPEOF_EMPTY_OBJECT,
        &[
            (
                "sig_t6",
                "(x: unknown) => x is object | null",
                "(x: unknown) => boolean",
            ),
            (
                "sig_t7",
                "(x: unknown) => x is object | null",
                "(x: unknown) => x is object",
            ),
            (
                "sig_t8",
                "(x: {} | null) => x is object | null",
                "(x: {}) => x is object",
            ),
            (
                "sig_t10",
                "(x: {}) => x is object",
                "(x: {}) => x is object",
            ),
        ],
    );
    let host = host_with(TYPEOF_EMPTY_OBJECT);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        super::signature_predicate_inference_tests::assert_complete(&host, root, "t4");
    }
}

const NON_UNION_MEMBER_EQUALITY: &str = r#"
export function d3(x: number[]) { return x.length === 0; }
export function d4(x: string) { return x.length === 0; }
export function d6(x: number[]) { if (x.length === 0) { return x; } throw 0; }
export function d7(x: { a: string }) { if (x.a === "q") { return x; } throw 0; }
export function sig_d3() { return d3; }
export function sig_d4() { return d4; }
"#;

/// A property equality narrows the property's parent only through a
/// DISCRIMINANT property of a union (`isMatchingReferenceDiscriminant`):
/// a parameter that is a union neither as declared nor as narrowed is
/// never narrowed, and its member is not read for it. Measured on 7.0.2,
/// identically without `strictNullChecks`: `d3` / `d4` return `boolean`
/// with no predicate, `d6` is `number[]`, `d7` `{ a: string; }`.
#[test]
fn a_member_equality_never_narrows_a_non_union_parent() {
    check_rows(
        NON_UNION_MEMBER_EQUALITY,
        &[
            ("d6", "number[]", "number[]"),
            ("d7", "{ a: string; }", "{ a: string; }"),
        ],
    );
    check_predicate_rows(
        NON_UNION_MEMBER_EQUALITY,
        &[
            (
                "sig_d3",
                "(x: number[]) => boolean",
                "(x: number[]) => boolean",
            ),
            ("sig_d4", "(x: string) => boolean", "(x: string) => boolean"),
        ],
    );
}

/// The lib environment the `length` rows register: the pinned lib's own
/// declarations of the members they read, narrowed to those members.
const LENGTH_LIB: &str = r#"
interface Array<T> { length: number; }
interface String { length: number; }
interface ArrayConstructor { isArray(arg: any): arg is any[]; }
declare var Array: ArrayConstructor;
"#;

const LENGTH_OVER_A_UNION: &str = r#"
export function orLen(x: string | number[]) { return !Array.isArray(x) || x.length === 0; }
export function eqLen(x: string | number[]) { return x.length === 0; }
export function retLen(x: string | number[]) { if (x.length === 0) return x; return x; }
export function retLenOnly(x: string | number[]) { if (x.length === 0) return x; throw 0; }
"#;

/// `x.length` over `string | number[]` reads each arm's `length` — the
/// string arm through its wrapper — and every arm projects `number`, so
/// `x.length === 0` discriminates no arm: the parent keeps both. Measured
/// on 7.0.2 under every `strictNullChecks` × `noImplicitAny` setting:
/// `orLen` and `eqLen` return `boolean` with no predicate, `retLen` and
/// `retLenOnly` return `string | number[]`.
#[test]
fn a_length_test_over_a_string_or_array_union_narrows_no_arm() {
    let rows = [
        ("orLen", "(x: string | number[]) => boolean"),
        ("eqLen", "(x: string | number[]) => boolean"),
        ("retLen", "(x: string | number[]) => string | number[]"),
        ("retLenOnly", "(x: string | number[]) => string | number[]"),
    ];
    let mut source = String::from(LENGTH_OVER_A_UNION);
    for (name, _) in &rows {
        source.push_str(&format!(
            "export function sig_{name}() {{ return {name}; }}\n"
        ));
    }
    let host =
        super::signature_predicate_inference_tests::host_with_lib_source(&source, LENGTH_LIB);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (name, printed) in rows {
            super::signature_predicate_inference_tests::assert_complete(&host, root, name);
            assert_prints(&host, root, &format!("sig_{name}"), printed);
            if printed.ends_with("=> boolean") {
                assert_predicate(&host, root, &format!("sig_{name}"), printed);
            }
        }
    }
}

const CONDITIONAL_ASSERTIONS: &str = r#"
function a(x: unknown): asserts x is string { if (typeof x !== "string") throw 0; }
function n(x: unknown): asserts x is number { if (typeof x !== "number") throw 0; }
function touch() {}
export function c1(x: string | number, c: boolean) { c ? (0, a(x)) : (0, a(x)); return x; }
export function c2(x: string | number, c: boolean, d: boolean) { c ? (d ? (0, a(x)) : (0, a(x))) : (0, a(x)); return x; }
export function c3(x: string | number, c: boolean) { (c ? (0, a(x)) : (0, a(x)), 0); return x; }
export function c4(x: string | number, c: boolean) { const u = c ? (0, a(x)) : (0, a(x)); return x; }
export function c5(x: string | number, c: boolean) { c ? ((0, a(x))) : (0, a(x)); return x; }
export function c6(x: string | number, c: boolean) { c ? (touch(), a(x)) : (0, a(x)); return x; }
export function c7(x: string | number) { typeof x === "string" ? (0, a(x)) : (0, a(x)); return x; }
export function o1(x: string | number, c: boolean) { c ? a(x) : a(x); return x; }
export function o2(x: string | number, c: boolean) { c ? (0, a(x)) : 0; return x; }
export function o3(x: string | number, c: boolean, d: boolean) { c ? (d ? (0, a(x)) : 0) : (0, a(x)); return x; }
export function o4(x: string | number, c: boolean) { c && (0, a(x)); return x; }
export function g1(x: string | number | boolean, c: boolean) { c ? (0, a(x)) : (0, n(x)); return x; }
"#;

/// The flow after a conditional joins its arms' ends, so an entered
/// `asserts` call every arm applies narrows past it — nested conditionals
/// included, the other operands of an arm aside. Measured on
/// 7.0.2 under every `strictNullChecks` × `noImplicitAny` setting: `c1` to
/// `c7` return `string`; a call that is an arm itself is not entered
/// (`o1`), and an arm that asserts nothing joins the unnarrowed path
/// (`o2`, `o3`, `o4`), each `string | number`. Arms that assert different
/// targets join to their union (`g1` is `string | number`).
#[test]
fn an_assertion_every_conditional_arm_applies_narrows_past_it() {
    let mut rows = Vec::new();
    for name in ["c1", "c2", "c3", "c4", "c5", "c6", "c7"] {
        rows.push((name, "string", "string"));
    }
    for name in ["o1", "o2", "o3", "o4", "g1"] {
        rows.push((name, "string | number", "string | number"));
    }
    check_rows(CONDITIONAL_ASSERTIONS, &rows);
}

/// Symbol abbreviation depends on spelling, not unrelated interning or query history.
#[test]
fn symbol_abbreviation_keeps_diagnostics_across_query_histories() {
    use super::signature_predicate_inference_tests::checker_report;

    let padding = "x".repeat(91);
    let mut source = String::from("declare function one(x: string): number;\n");
    for i in 0..12 {
        source.push_str(&format!("declare const s{i}: unique symbol;\n"));
    }
    source.push_str(&format!(
        "export function target(x: {{ padding: \"{padding}\"; "
    ));
    for i in 0..12 {
        source.push_str(&format!("[s{i}]: number; "));
    }
    source.push_str("last: string }) { return [one(x), one(x)]; }\n");
    for i in 0..128 {
        source.push_str(&format!("declare const u{i}: unique symbol;\n"));
    }
    source.push_str("export function unrelated(x: { ");
    for i in 0..128 {
        source.push_str(&format!("[u{i}]: {i}; "));
    }
    source.push_str("}) { return x; }\n");
    let message = format!("error TS2345: Argument of type '{{ padding: \"{padding}\"; [s0]: number; [s1]: number; [s2]: number; [s3]: number; [s4]: number; ... 7 more ...; last: string; }}' is not assignable to parameter of type 'string'.");
    let expected: Vec<_> = source
        .match_indices("one(x)")
        .map(|(start, _)| {
            let before = &source[..start + "one(".len()];
            let line = before.matches('\n').count() + 1;
            let column = before.len() - before.rfind('\n').unwrap();
            format!("({line},{column}): {message}")
        })
        .collect();
    assert_eq!(expected.len(), 2, "the fixture has two authored call sites");
    for prewarm in [false, true] {
        let host = host_with(&source);
        for pass in ["cold", "warm"] {
            for root in [STRICT_ROOT, LOOSE_ROOT] {
                if prewarm {
                    assert!(checker_report(&host, root, "unrelated", &source).is_empty());
                }
                assert_eq!(
                    checker_report(&host, root, "target", &source),
                    expected,
                    "{pass} {root}, unrelated prewarm: {prewarm}"
                );
                assert_prints(&host, root, "target", "number[]");
            }
        }
    }
}

/// Printable residual kinds retain the call's diagnostic through cold and warm reads.
#[test]
fn printable_residual_types_keep_call_diagnostics() {
    let source = r#"declare function one(x: string): number;
interface Merged { a: number }
interface Merged { b: string }
export function merged(x: Merged) { return one(x); }
export function spread(x: { a: number; b: string }) { const y = { ...x }; return one(y); }
export function constrained<T>(x: { padding: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; value: T extends { v: infer U extends string } ? U : number; tail: boolean }) { return one(x); }
declare const key: unique symbol;
export function symbol_short(x: { [key]: number }) { return one(x); }
export function symbol_wide(x: { padding: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; [key]: number; tail: boolean; last: string }) { return one(x); }
export function mapped<T>(x: { padding: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; value: { [K in keyof T]: string }; tail: boolean }) { return one(x); }
export function constrained_short<T>(x: { value: T extends { v: infer U extends string } ? U : number }) { return one(x); }
export function template_infer<T>(x: { value: T extends `${infer U extends string}` ? U : number }) { return one(x); }
export function rest_infer<T>(x: { value: T extends [...infer U extends unknown[]] ? U : number }) { return one(x); }
export function indexed<T extends { [key]: number }>(x: { padding: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; value: T[typeof key]; tail: boolean }) { return one(x); }
export function mapped_template<T>(x: { padding: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"; value: { [K in keyof T]: { p0: 0; p1: 1; p2: 2; p3: 3; p4: 4; p5: 5; p6: 6; p7: 7; p8: 8; p9: 9 } }; tail: boolean }) { return one(x); }
declare namespace ns { const key: unique symbol; }
export function qualified_symbol(x: { [ns.key]: number }) { return one(x); }
export function computed_literal(x: { ["a-b"]: number }) { return one(x); }
export function generic_spread<T extends object>(x: T) { const y = { ...x, extra: 1 }; return one(y); }
export function generic_between<T extends object>(x: T) { const y = { before: "a", ...x, after: true }; return one(y); }
export function nested_generic<T extends object>(x: T) { const y = { ...{ ...x, inner: 1 }, outer: true }; return one(y); }
type StringAlias = string;
export function infer_alias<T>(x: { value: T extends `${infer U extends StringAlias}` ? U : number }) { return one(x); }
export function infer_repeated<T>(x: { value: T extends [`${infer U extends string}`, { [K in infer U extends string]: number }] ? U : number }) { return one(x); }
export function mapped_infer<T>(x: { value: { [K in keyof T]: string } extends { [K in keyof T]: infer U extends string } ? U : number }) { return one(x); }
"#;
    let rows = [
        ("merged", "(4,48): error TS2345: Argument of type 'Merged' is not assignable to parameter of type 'string'."),
        ("spread", "(5,86): error TS2345: Argument of type '{ a: number; b: string; }' is not assignable to parameter of type 'string'."),
        ("constrained", "(6,271): error TS2345: Argument of type '{ padding: \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"; value: T extends { ...; } ? U : number; tail: boolean; }' is not assignable to parameter of type 'string'."),
        ("symbol_short", "(8,65): error TS2345: Argument of type '{ [key]: number; }' is not assignable to parameter of type 'string'."),
        ("symbol_wide", "(9,236): error TS2345: Argument of type '{ padding: \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"; [key]: number; tail: boolean; last: string; }' is not assignable to parameter of type 'string'."),
        ("mapped", "(10,240): error TS2345: Argument of type '{ padding: \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"; value: { [K in keyof T]: string; }; tail: boolean; }' is not assignable to parameter of type 'string'."),
        ("constrained_short", "(11,119): error TS2345: Argument of type '{ value: T extends { v: infer U extends string; } ? U : number; }' is not assignable to parameter of type 'string'."),
        ("template_infer", "(12,114): error TS2345: Argument of type '{ value: T extends `${infer U}` ? U : number; }' is not assignable to parameter of type 'string'."),
        ("rest_infer", "(13,113): error TS2345: Argument of type '{ value: T extends [...infer U] ? U : number; }' is not assignable to parameter of type 'string'."),
        ("indexed", "(14,254): error TS2345: Argument of type '{ padding: \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"; value: T[unique symbol]; tail: boolean; }' is not assignable to parameter of type 'string'."),
        ("mapped_template", "(15,315): error TS2345: Argument of type '{ padding: \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"; value: { [K in keyof T]: { p0: 0; p1: 1; p2: 2; ... 6 more ...; p9: 9; }; }; tail: boolean; }' is not assignable to parameter of type 'string'."),
        ("qualified_symbol", "(17,72): error TS2345: Argument of type '{ [ns.key]: number; }' is not assignable to parameter of type 'string'."),
        ("computed_literal", "(18,71): error TS2345: Argument of type '{ \"a-b\": number; }' is not assignable to parameter of type 'string'."),
        ("generic_spread", "(19,99): error TS2345: Argument of type 'T & { extra: number; }' is not assignable to parameter of type 'string'."),
        ("generic_between", "(20,116): error TS2345: Argument of type '{ before: string; } & T & { after: boolean; }' is not assignable to parameter of type 'string'."),
        ("nested_generic", "(21,119): error TS2345: Argument of type 'T & { inner: number; outer: boolean; }' is not assignable to parameter of type 'string'."),
        ("infer_alias", "(23,116): error TS2345: Argument of type '{ value: T extends `${infer U}` ? U : number; }' is not assignable to parameter of type 'string'."),
        ("infer_repeated", "(24,159): error TS2345: Argument of type '{ value: T extends [`${infer U}`, { [K in infer U]: number; }] ? U : number; }' is not assignable to parameter of type 'string'."),
        ("mapped_infer", "(25,152): error TS2345: Argument of type '{ value: { [K in keyof T]: string; } extends { [K in keyof T]: infer U; } ? U : number; }' is not assignable to parameter of type 'string'."),
    ];
    let host = host_with(source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (function, expected) in rows {
                let reported = super::signature_predicate_inference_tests::checker_report(
                    &host, root, function, source,
                );
                if reported != [expected] {
                    mismatches.push(format!(
                        "{pass} {root} {function}: expected {expected:?}, got {reported:?}"
                    ));
                }
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn spread_and_computed_key_diagnostics_preserve_evaluated_types() {
    let padding = "x".repeat(130);
    let source = format!(
        r#"declare function one(x: string): number;
declare const k: "a";
enum E {{ A = "a", N = 1 }}
export function repeated<T extends object>(x: T) {{ const y = {{ ...x, a: 1, ...x, b: true }}; return one(y); }}
export function overwritten<T extends object>(x: T) {{ const a = {{ p: 1 }}; const b = {{ p: true }}; const y = {{ ...x, ...a, ...x, ...b }}; return one(y); }}
export function literal(x: {{ [k]: number }}) {{ return one(x); }}
export function enumeration(x: {{ [E.A]: number; [E.N]: string }}) {{ return one(x); }}
export function wide(x: {{ padding: "{padding}"; [k]: number; tail: boolean; last: string }}) {{ return one(x); }}
export function dynamic(x: any) {{ const y = {{ ...x }}; return one(y); }}
export function dynamic_before(x: any) {{ const y = {{ p: 1, ...x }}; return one(y); }}
export function dynamic_after(x: any) {{ const y = {{ ...x, p: 1 }}; return one(y); }}
export function closed(x: {{ a: number }}) {{ const y = {{ ...x }}; return one(y); }}
export function enum_value(x: {{ [E.A]: E.A }}) {{ return one(x); }}
export function dynamic_nested(x: any) {{ const y = {{ ...{{ ...x, inner: 1 }}, outer: true }}; return one(y); }}
export function property_dynamic(x: {{ a: any }}) {{ const y = {{ ...x }}; return one(y); }}
export function enum_wide(x: {{ padding: "{padding}"; [E.A]: number; tail: boolean; last: string }}) {{ return one(x); }}
"#
    );
    let expected = [
        (
            "repeated",
            Some("T & { a: number; b: boolean; }".to_owned()),
        ),
        ("overwritten", Some("T & { p: boolean; }".to_owned())),
        ("literal", Some("{ a: number; }".to_owned())),
        ("enumeration", Some("{ a: number; 1: string; }".to_owned())),
        (
            "wide",
            Some(format!(
                "{{ padding: \"{padding}\"; a: number; tail: boolean; last: string; }}"
            )),
        ),
        ("dynamic", None),
        ("dynamic_before", None),
        ("dynamic_after", None),
        ("closed", Some("{ a: number; }".to_owned())),
        ("enum_value", Some("{ a: E.A; }".to_owned())),
        ("dynamic_nested", None),
        ("property_dynamic", Some("{ a: any; }".to_owned())),
        (
            "enum_wide",
            Some(format!(
                "{{ padding: \"{padding}\"; a: number; tail: boolean; last: string; }}"
            )),
        ),
    ];
    let host = host_with(&source);
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            for (name, ty) in &expected {
                let reports = super::signature_predicate_inference_tests::checker_report(
                    &host, root, name, &source,
                );
                let expected: Vec<_> = ty.iter().map(|ty| {
                    let line = source.lines().position(|line| line.starts_with(&format!("export function {name}<")) || line.starts_with(&format!("export function {name}("))).unwrap();
                    let text = source.lines().nth(line).unwrap();
                    let column = text.rfind("one(").unwrap() + 5;
                    format!("({},{}): error TS2345: Argument of type '{ty}' is not assignable to parameter of type 'string'.", line + 1, column)
                }).collect();
                if reports != expected {
                    mismatches.push(format!(
                        "{pass} {root} {name}: got {reports:?}, expected {expected:?}"
                    ));
                }
                assert_prints(&host, root, name, "number");
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn elaborated_and_overloaded_overflows_keep_call_and_argument_reports() {
    let nest =
        |leaf: &str, count: usize| format!("{}{leaf}{}", "{ e: ".repeat(count), " }".repeat(count));
    let clipped = format!("{}{{ ...", "{ e: ".repeat(63));
    for (name, overload, elaborated, count) in [
        ("elaborated", false, true, 100),
        ("overloaded", true, false, 100),
        ("part_overflow", false, true, 101),
        ("overload_elaborated", true, true, 100),
    ] {
        // A context-sensitive sibling exercises the supported overload
        // diagnostic producer while the first argument owns the overflow.
        let callback = if overload {
            ", cb: (v: number) => number"
        } else {
            ""
        };
        let callback_argument = if overload { ", v => v" } else { "" };
        let source = format!("declare const x: {};\ndeclare function f(arg: {}{callback}): number;\n{}export function {name}() {{ return f({}{callback_argument}); }}\n",
            nest("{ x: number }", count), nest("{ y: number }", count),
            if overload { format!("declare function f(arg: {}{callback}, extra?: number): number;\n", nest("{ y: number }", count)) } else { String::new() },
            if elaborated { "{ e: x.e }" } else { "x" });
        let line = source.lines().count();
        let body = source.lines().last().unwrap();
        let call = body.find("f(").unwrap() + 1;
        let argument = call + 2;
        let overflow =
            format!("Excessive stack depth comparing types '{clipped}' and '{clipped}'.");
        let mut expected = vec![format!("({line},{}): error TS2321: {overflow}", call)];
        if overload && !elaborated {
            expected.push(format!("({line},{argument}): error TS2769: No overload matches this call.\n  The last overload gave the following error.\n    {overflow}"));
        } else if count == 101 {
            expected.push(format!(
                "({line},{}): error TS2321: {overflow}",
                argument + 2
            ));
        } else {
            let message = format!("Type '{clipped}' is not assignable to type '{clipped}'. Two different types with this name exist, but they are unrelated.\n  The types of '{}' are incompatible between these types.\n    Property 'y' is missing in type '{{ x: number; }}' but required in type '{{ y: number; }}'.", vec!["e"; 99].join("."));
            if overload {
                expected.push(format!("({line},{}): error TS2769: No overload matches this call.\n  The last overload gave the following error.\n    {}", argument + 2, message.replace('\n', "\n    ")));
            } else {
                expected.push(format!(
                    "({line},{}): error TS2719: {message}",
                    argument + 2
                ));
            }
        }
        let host = host_with(&source);
        for pass in ["cold", "warm"] {
            for root in [STRICT_ROOT, LOOSE_ROOT] {
                assert_eq!(
                    super::signature_predicate_inference_tests::checker_report(
                        &host, root, name, &source
                    ),
                    expected,
                    "{pass} {root} {name}"
                );
                if overload {
                    super::signature_predicate_inference_tests::observe(
                        &host,
                        root,
                        name,
                        |_, degradation, _| {
                            assert!(
                                degradation.is_some(),
                                "overload recovery retains its existing typed gap"
                            )
                        },
                    );
                } else {
                    assert_prints(&host, root, name, "number");
                }
            }
        }
    }
}
