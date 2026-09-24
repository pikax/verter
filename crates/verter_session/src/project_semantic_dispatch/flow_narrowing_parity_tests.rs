//! Narrowing forms the flow lane models as the checker does, each read
//! back as a flow-return answer and, where the form is a returned test, as
//! the type predicate the checker infers from it.
//!
//! Every expected answer is measured on the pinned TypeScript 7.0.2: the
//! strict column is the function's `.d.ts` line (`tsc --declaration
//! --emitDeclarationOnly --strict`), the loose column the same under
//! `--strictNullChecks false`, read from a TS2322 message quoting the type
//! where the loose `.d.ts` reprints an authored annotation instead. A
//! branch that does not return `throw`s, so each answer is the narrowed
//! read alone, never a join.

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
/// [`a_type_parameter_narrows_through_its_constraint`]). Two calls degrade: `isFoo(o["x"])`
/// narrows the element access (`c14` is `Foo`) and `isFoo(o?.x)` the
/// optional chain's root (`c18` is `{ x: Foo | Bar; }`), references this
/// half does not carry.
#[test]
fn a_call_to_a_predicate_signature_narrows_its_argument() {
    check_rows(CALL_PREDICATES, CALL_ROWS);
    check_predicate_rows(CALL_PREDICATES, CALL_PREDICATE_ROWS);
    let host = host_with(CALL_PREDICATES);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for function in ["c14", "c18"] {
            super::signature_predicate_inference_tests::assert_degrades(&host, root, function);
        }
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
"#;

/// A call its only candidate does not accept continues with that
/// candidate as the checker's error-recovery candidate
/// (`getCandidateForOverloadFailure`), re-inferred from the arguments, and
/// the diagnostic the checker reports rides with the answer. Measured on
/// 7.0.2, identically without `strictNullChecks`: `takes(u)` is `A`
/// (TS2345: the inferred `x is A` does not accept `B`'s predicate);
/// `two(x, y)` over `{ a: 1 }` and `{ b: 1 }` is `{ a: 1 }` — the
/// candidates' common supertype, which `y` then fails — where the checker
/// prints the argument failure's elaboration, TS2741 ("Property 'a' is
/// missing");
/// `con(1)` is `string` — the inference violating `T extends string`
/// takes the constraint (TS2345); `ctx(1, (v: string) => {})` is `string`
/// (TS2345).
///
/// A call to a lone NON-generic signature is answered from its declared
/// return without resolving the call — the checker's answer whatever the
/// arguments: `one(x)` is `number` (TS2345), `one()` and `one("a", "b")`
/// are `number` (TS2554), `rest()` is `number` (TS2555), `opt()` is
/// `number` (TS2554). No applicability check runs there, so no diagnostic
/// rides those answers.
///
/// A generic candidate whose inference relation fails (`box(x)`, the
/// checker's `unknown`) and a call over several overloads (`ov(x)`, the
/// checker's `never` with TS2769) keep the typed call gap: the first
/// re-infers from a relation this executor abandoned, the second answers
/// from a signature combining every overload.
#[test]
fn a_call_its_only_candidate_rejects_answers_the_checkers_recovery() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation,
    };
    let rows = [
        ("tu", "A", CheckerDiagnosticCode::ArgumentNotAssignable),
        (
            "tab",
            "{ a: 1; }",
            CheckerDiagnosticCode::ArgumentNotAssignable,
        ),
        ("c1", "string", CheckerDiagnosticCode::ArgumentNotAssignable),
        ("x1", "string", CheckerDiagnosticCode::ArgumentNotAssignable),
    ];
    let host = host_with(CALL_RECOVERY);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        for (function, printed, code) in rows {
            assert_prints(&host, root, function, printed);
            assert_eq!(
                super::signature_predicate_inference_tests::checker_diagnostics(
                    &host, root, function
                ),
                vec![CheckerDiagnostic {
                    code,
                    operation: CheckerDiagnosticOperation::CallResolution,
                }],
                "`{function}` in {root} carries the checker's TS{}",
                code.code()
            );
        }
        for function in ["o1", "o2", "o3", "r0", "op0"] {
            assert_prints(&host, root, function, "number");
        }
        for function in ["bx", "v1"] {
            super::signature_predicate_inference_tests::assert_degrades(&host, root, function);
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
