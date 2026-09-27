//! A mapped type declared over `keyof T` for a type parameter `T` — the
//! homomorphic utilities `Partial`, `Required` and `Readonly`, and any
//! alias of the same form — instantiates the way TypeScript's
//! `instantiateMappedType` does: a union maps each constituent, a primitive
//! passes through, and an array or tuple maps element-wise with the
//! modifiers applied to its elements. A mapping written over a concrete
//! type maps that type's own keys.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit` (strict by default),
//! then with `--strictNullChecks false` and with
//! `--exactOptionalPropertyTypes`: `declare const v: <probe>; export const
//! s: null = v;` read off the TS2322 message.

use super::checker_probe_lane_tests::{
    evaluated_mismatches, mismatches, mismatches_in, ProbeProject,
};

const MAPPED: &str = "\
interface Face { a: 1 }
type Obj = { o: 1 };
type U = Face | Obj;
type Mp<T> = { [K in keyof T]: T[K] };
type MpS<T> = { [K in keyof T]: string };
type MpR<T> = { [K in keyof T as `x${K & string}`]: T[K] };
type P<T> = Partial<T>;
type Tup = [1, 'two', 3?];
type RoT = readonly [1, 2];
declare function g<T>(x: T): { [K in keyof T]: T[K] };
export function callG() { return g('s' as string); }
export function callGA() { return g([1] as number[]); }
declare function gp<T>(x: T): { [K in keyof T]?: T[K] };
export function callGP() { return gp([1] as number[]); }
function h<T>(x: T) { const y: { [K in keyof T]: T[K] } = null!; return y; }
export function callH() { return h('s' as string); }
";

/// A primitive passes through, with or without an `as` clause, and `never`
/// is `never`.
///
/// Measured on TypeScript 7.0.2: `Partial<string>`, `Mp<string>`,
/// `P<string>` and `MpR<string>` are `string`; `Partial<undefined>` is
/// `undefined`, `Partial<symbol>` `symbol`, `Partial<bigint>` `bigint`,
/// `Partial<object>` `object` and `Partial<1>` `1`; `Partial<null>`,
/// `Partial<never>` and `Mp<never>` are assignable to `null` (they are
/// `null` and `never`); `Partial<boolean>` prints as the application and
/// is mutually assignable with `boolean`.
#[test]
fn a_primitive_passes_through() {
    let failures = mismatches(
        MAPPED,
        &[
            ("Partial<string>", "string"),
            ("Mp<string>", "string"),
            ("P<string>", "string"),
            ("MpR<string>", "string"),
            ("Partial<undefined>", "undefined"),
            ("Partial<null>", "null"),
            ("Partial<symbol>", "symbol"),
            ("Partial<bigint>", "bigint"),
            ("Partial<object>", "object"),
            ("Partial<1>", "1"),
            ("Partial<boolean>", "boolean"),
            ("Partial<never>", "never"),
            ("Mp<never>", "never"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A mapping written inline in a generic signature, or in the body of a
/// generic function, instantiates the same way at a call.
///
/// Measured on TypeScript 7.0.2 over `declare function g<T>(x: T): { [K in
/// keyof T]: T[K] }` and its `?` twin `gp`: `g('s' as string)` returns
/// `string`, `g([1] as number[])` `number[]`, and `gp([1] as number[])`
/// `(number | undefined)[]`; over `function h<T>(x: T) { const y: { [K in
/// keyof T]: T[K] } = null!; return y; }`, `h('s' as string)` returns
/// `string`.
#[test]
fn a_mapping_in_a_generic_signature_instantiates_at_a_call() {
    let failures = mismatches(
        MAPPED,
        &[
            ("ReturnType<typeof callG>", "string"),
            ("ReturnType<typeof callGA>", "number[]"),
            ("ReturnType<typeof callGP>", "(number | undefined)[]"),
            ("ReturnType<typeof callH>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union maps each constituent: `Partial<Face | Obj>` is `Partial<Face> |
/// Partial<Obj>`, not the mapping of the constituents' common keys. The
/// checker prints the application by name, so the comparison reads the
/// value the application evaluates to, against a type the checker holds
/// mutually assignable with it; `unknown` maps to `{}`.
///
/// Measured on TypeScript 7.0.2: each `X extends Y` and `Y extends X` is
/// `"y"` for `Partial<Face | Obj>`, `Partial<U>` and `P<Face | Obj>`
/// against `{ a?: 1 } | { o?: 1 }`, `Mp<Face | Obj>` against `{ a: 1 } | {
/// o: 1 }`, `MpS<Face | Obj>` against `{ a: string } | { o: string }`,
/// `Partial<Face | string>` against `{ a?: 1 } | string`, `Required<{ a?: 1
/// } | { b?: 2 }>` against `{ a: 1 } | { b: 2 }`; `Readonly<Face | Obj>
/// extends { readonly a: 1 } | { readonly o: 1 }` is `"y"`; `keyof
/// Partial<Face | Obj>` and `keyof Partial<unknown>` are `never`.
#[test]
fn a_union_maps_each_constituent() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("Partial<Face | Obj>", "{ a?: 1; } | { o?: 1; }"),
            ("Partial<U>", "{ a?: 1; } | { o?: 1; }"),
            ("P<Face | Obj>", "{ a?: 1; } | { o?: 1; }"),
            ("Mp<Face | Obj>", "{ a: 1; } | { o: 1; }"),
            ("MpS<Face | Obj>", "{ a: string; } | { o: string; }"),
            ("Partial<Face | string>", "{ a?: 1; } | string"),
            ("Required<{ a?: 1 } | { b?: 2 }>", "{ a: 1; } | { b: 2; }"),
            (
                "Readonly<Face | Obj>",
                "{ readonly a: 1; } | { readonly o: 1; }",
            ),
            ("Partial<unknown>", "{}"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An array maps its element and a tuple each element, the modifiers
/// applying to the elements: `+?` makes every element optional and adds
/// `undefined` (unless `exactOptionalPropertyTypes` carries it on the
/// optional flag), `-?` removes both, `readonly` makes the array or tuple
/// readonly. With `strictNullChecks` off no `undefined` is added.
///
/// Measured on TypeScript 7.0.2, strict / `--strictNullChecks false` /
/// `--exactOptionalPropertyTypes`: `Partial<number[]>[number]` is `number |
/// undefined` / `number` / `number | undefined`;
/// `Required<(number | undefined)[]>[number]` is `number` throughout;
/// `MpS<number[]>[number]` is `string`; `Partial<Tup>[0]` is `1 |
/// undefined` / `1` / `1 | undefined` and `[1]` `"two" | undefined` /
/// `"two"` / `"two" | undefined`; `Required<Tup>[2]` is `3`;
/// `Readonly<Tup>[2]` and `Mp<Tup>[2]` are `3 | undefined` / `3` / `3 |
/// undefined`; `MpS<Tup>[0]` is `string` and `MpS<Tup>[2]` `string |
/// undefined` / `string` / `string | undefined`; throughout,
/// `Readonly<number[]> extends number[]` is `"n"` and the converse `"y"`,
/// `Partial<number[]>` and `(number | undefined)[]` are mutually
/// assignable, `Readonly<Tup> extends Tup` is `"n"` and the converse
/// `"y"`, `Partial<RoT> extends readonly [1?, 2?]` is `"y"` while
/// `Partial<RoT> extends [1?, 2?]` is `"n"`, `[] extends Partial<Tup>` is
/// `"y"` while `[] extends Tup` is `"n"`, and `Required<[1?]> extends [1]`
/// is `"y"`.
#[test]
fn an_array_or_tuple_maps_element_wise() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let exact = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ambient_lib: None,
    };
    for (project, strict_null_checks) in [(strict, true), (loose, false), (exact, true)] {
        let read = |on: &'static str, off: &'static str| if strict_null_checks { on } else { off };
        let failures = mismatches_in(
            project,
            MAPPED,
            &[
                (
                    "Partial<number[]>[number]",
                    read("number | undefined", "number"),
                ),
                ("Required<(number | undefined)[]>[number]", "number"),
                ("MpS<number[]>[number]", "string"),
                ("Partial<Tup>[0]", read("1 | undefined", "1")),
                ("Partial<Tup>[1]", read("\"two\" | undefined", "\"two\"")),
                ("Required<Tup>[2]", "3"),
                ("Readonly<Tup>[2]", read("3 | undefined", "3")),
                ("Mp<Tup>[2]", read("3 | undefined", "3")),
                ("MpS<Tup>[0]", "string"),
                ("MpS<Tup>[2]", read("string | undefined", "string")),
                ("Readonly<number[]> extends number[] ? 'y' : 'n'", "\"n\""),
                ("number[] extends Readonly<number[]> ? 'y' : 'n'", "\"y\""),
                (
                    "Partial<number[]> extends (number | undefined)[] ? 'y' : 'n'",
                    "\"y\"",
                ),
                (
                    "(number | undefined)[] extends Partial<number[]> ? 'y' : 'n'",
                    "\"y\"",
                ),
                ("Readonly<Tup> extends Tup ? 'y' : 'n'", "\"n\""),
                ("Tup extends Readonly<Tup> ? 'y' : 'n'", "\"y\""),
                (
                    "Partial<RoT> extends readonly [1?, 2?] ? 'y' : 'n'",
                    "\"y\"",
                ),
                ("Partial<RoT> extends [1?, 2?] ? 'y' : 'n'", "\"n\""),
                ("[] extends Partial<Tup> ? 'y' : 'n'", "\"y\""),
                ("[] extends Tup ? 'y' : 'n'", "\"n\""),
                ("Required<[1?]> extends [1] ? 'y' : 'n'", "\"y\""),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// An OPTIONAL tuple element's type is its written type plus `undefined`
/// under `strictNullChecks`, so the relation compares elements that way;
/// under `exactOptionalPropertyTypes` the implied `undefined` is the
/// missing type, which an explicit `undefined` does not satisfy.
///
/// Measured on TypeScript 7.0.2, strict / `--strictNullChecks false` /
/// `--exactOptionalPropertyTypes`: `[(1 | undefined)?] extends [1?]` is
/// `"y"` / `"y"` / `"n"`; `[1?] extends [(1 | undefined)?]` is `"y"`
/// throughout; `[1 | undefined] extends [1?]` and `[undefined] extends
/// [1?]` are `"y"` / `"y"` / `"n"`; `[1?] extends [1]` is `"n"` throughout.
#[test]
fn an_optional_tuple_element_relates_with_its_implied_undefined() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let exact = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ambient_lib: None,
    };
    for (project, exact_optional) in [(strict, false), (loose, false), (exact, true)] {
        let read =
            |plain: &'static str, exact: &'static str| if exact_optional { exact } else { plain };
        let failures = mismatches_in(
            project,
            MAPPED,
            &[
                (
                    "[(1 | undefined)?] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                ("[1?] extends [(1 | undefined)?] ? 'y' : 'n'", "\"y\""),
                (
                    "[1 | undefined] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                (
                    "[undefined] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                ("[1?] extends [1] ? 'y' : 'n'", "\"n\""),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
