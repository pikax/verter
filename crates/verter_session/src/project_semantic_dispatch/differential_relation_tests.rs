//! Differential probes of the checker's assignability relation, each a
//! `[S] extends [T] ? 1 : 2` in TYPE position over a small fixture: object
//! types, unions, intersections, private and protected members, generic
//! applications and signatures, enum and template literal types.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact probe
//! with `tsc --ignoreConfig --declaration --emitDeclarationOnly --strict
//! --noErrorTruncation` under each `strictNullChecks` × `noImplicitAny`
//! setting: `declare const p: <probe>; const s: never = p;` read off the
//! TS2322 message. A row with one answer answers alike in the four settings.
//! An ignored test asserts the measured answer for probes the lane does not
//! yet answer as the checker does; "wrong-but-clean" marks a lane answer
//! published complete and undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// Interfaces, optional, readonly and method members, call and construct
/// signatures.
const OBJECTS: &str = r##"
interface P { x: number; y: number }
interface P3 extends P { z: number }
interface OptX { x?: number }
interface RoX { readonly x: number }
interface Named { name: string }
type Empty = {};
interface Fn { (a: string): number }
interface Ctor { new (a: string): Named }
interface M { m(a: string): void }
interface Mv { m: (a: "x") => void }
"##;

/// An object type relates member by member: optional and readonly modifiers,
/// index signatures, call and construct signatures, arrays and tuples, `{}` and
/// `object`.
#[test]
fn object_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(OBJECTS);
    let mut failures = matrix.types(&[
        ("[P3] extends [P] ? 1 : 2", "1"),
        ("[P] extends [P3] ? 1 : 2", "2"),
        ("[{ x: number; y: number }] extends [P] ? 1 : 2", "1"),
        ("[{ x: 1; y: 2 }] extends [P] ? 1 : 2", "1"),
        ("[{ x: number }] extends [OptX] ? 1 : 2", "1"),
        ("[OptX] extends [{ x: number }] ? 1 : 2", "2"),
        ("[{}] extends [OptX] ? 1 : 2", "1"),
        ("[RoX] extends [{ x: number }] ? 1 : 2", "1"),
        ("[{ x: number }] extends [RoX] ? 1 : 2", "1"),
        ("[P] extends [Empty] ? 1 : 2", "1"),
        ("[string] extends [Empty] ? 1 : 2", "1"),
        ("[number] extends [object] ? 1 : 2", "2"),
        ("[P] extends [object] ? 1 : 2", "1"),
        ("[Fn] extends [object] ? 1 : 2", "1"),
        ("[() => void] extends [Function] ? 1 : 2", "1"),
        ("[Fn] extends [(a: string) => number] ? 1 : 2", "1"),
        ("[(a: string) => number] extends [Fn] ? 1 : 2", "1"),
        ("[(a: string) => 1] extends [Fn] ? 1 : 2", "1"),
        ("[(a: string) => string] extends [Fn] ? 1 : 2", "2"),
        ("[() => number] extends [Fn] ? 1 : 2", "1"),
        (
            "[(a: string, b: number) => number] extends [Fn] ? 1 : 2",
            "2",
        ),
        ("[(a: \"x\") => number] extends [Fn] ? 1 : 2", "2"),
        ("[(a: string | number) => number] extends [Fn] ? 1 : 2", "1"),
        ("[Ctor] extends [new (a: string) => Named] ? 1 : 2", "1"),
        ("[Ctor] extends [Fn] ? 1 : 2", "2"),
        ("[M] extends [Mv] ? 1 : 2", "1"),
        (
            "[{ a: { b: string } }] extends [{ a: { b: string | number } }] ? 1 : 2",
            "1",
        ),
        (
            "[{ a: { b: string | number } }] extends [{ a: { b: string } }] ? 1 : 2",
            "2",
        ),
        (
            "[{ a: string; b: number }] extends [{ a: string } | { b: string }] ? 1 : 2",
            "1",
        ),
        (
            "[{ x: number; y?: string }] extends [{ x: number; y?: number }] ? 1 : 2",
            "2",
        ),
        (
            "[{ [k: string]: number }] extends [{ a: number }] ? 1 : 2",
            "2",
        ),
        (
            "[{ [k: string]: number }] extends [{ a?: number }] ? 1 : 2",
            "1",
        ),
        (
            "[{ a: number }] extends [{ [k: string]: number }] ? 1 : 2",
            "1",
        ),
        (
            "[{ a: 1; b: \"s\" }] extends [{ [k: string]: number }] ? 1 : 2",
            "2",
        ),
        (
            "[{ 0: string }] extends [{ [n: number]: string }] ? 1 : 2",
            "1",
        ),
        ("[string[]] extends [readonly string[]] ? 1 : 2", "1"),
        ("[readonly string[]] extends [string[]] ? 1 : 2", "2"),
        (
            "[[string, number]] extends [(string | number)[]] ? 1 : 2",
            "1",
        ),
        (
            "[(string | number)[]] extends [[string, number]] ? 1 : 2",
            "2",
        ),
        (
            "[[string, number]] extends [[string, number?]] ? 1 : 2",
            "1",
        ),
        (
            "[[string, number?]] extends [[string, number]] ? 1 : 2",
            "2",
        ),
        ("[[string]] extends [[string, ...number[]]] ? 1 : 2", "1"),
        (
            "[[string, 1, 2]] extends [[string, ...number[]]] ? 1 : 2",
            "1",
        ),
        ("[[]] extends [string[]] ? 1 : 2", "1"),
        ("[never[]] extends [string[]] ? 1 : 2", "1"),
        ("[readonly [1, 2]] extends [readonly number[]] ? 1 : 2", "1"),
        ("[readonly [1, 2]] extends [number[]] ? 1 : 2", "2"),
        ("[[1, 2]] extends [{ length: 3 }] ? 1 : 2", "2"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Type("[null] extends [Empty] ? 1 : 2"), "2", "1"),
        (Read::Type("[undefined] extends [Empty] ? 1 : 2"), "2", "1"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A target member declared as a METHOD compares its parameters bivariantly
/// under `strictFunctionTypes` (`strictVariance` excludes method signatures),
/// so a function-typed property with a narrower parameter fits it.
/// Wrong-but-clean.
///
/// What the lane gives:
/// - `[Mv] extends [M] ? 1 : 2`: the checker answers `1`; the lane measured
///   `2`.
#[test]
#[ignore = "a method member's parameters relate bivariantly under strictFunctionTypes"]
fn wrong_clean_a_function_property_relates_to_a_method_member_bivariantly() {
    let matrix = Matrix::new(OBJECTS);
    let failures = matrix.types(&[("[Mv] extends [M] ? 1 : 2", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Only an object LITERAL type gets an implicit index signature; an interface
/// does not, so `P` is not below `{ [k: string]: number }`. Wrong-but-clean.
///
/// What the lane gives:
/// - `[P] extends [{ [k: string]: number }] ? 1 : 2`: the checker answers `2`;
///   the lane measured `1`.
#[test]
#[ignore = "an interface has no implicit index signature"]
fn wrong_clean_an_interface_is_not_below_an_implicit_string_index_signature() {
    let matrix = Matrix::new(OBJECTS);
    let failures = matrix.types(&[("[P] extends [{ [k: string]: number }] ? 1 : 2", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A tuple type carries its own members: `length` as its possible lengths
/// and each position as a numeric-key property, so `[1, 2]` relates to `{
/// length: 2 }` and `{ 0: 1 }`, `[1, 2?]` to `{ length: 1 | 2 }`, and neither
/// a wrong length, a wrong position nor a rest tuple's `number` length
/// fits a literal one (TypeScript 7.0.2, all four settings alike).
#[test]
fn a_tuple_relates_as_the_checker_relates_its_own_members() {
    let matrix = Matrix::new(OBJECTS);
    let failures = matrix.types(&[
        ("[[1, 2]] extends [{ length: 2 }] ? 1 : 2", "1"),
        ("[[1, 2?]] extends [{ length: 1 | 2 }] ? 1 : 2", "1"),
        ("[[1, 2]] extends [{ 0: 1 }] ? 1 : 2", "1"),
        ("[[1, 2]] extends [{ length: 3 }] ? 1 : 2", "2"),
        ("[[1, 2]] extends [{ 1: 3 }] ? 1 : 2", "2"),
        ("[[1, ...number[]]] extends [{ length: 1 }] ? 1 : 2", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Literal unions and discriminated object unions.
const UNIONS: &str = r##"
type AB = "a" | "b";
type ABC = "a" | "b" | "c";
interface Cat { kind: "cat"; meow(): void }
interface Dog { kind: "dog"; bark(): void }
type Pet = Cat | Dog;
type Shape = { kind: "circle"; r: number } | { kind: "square"; s: number };
"##;

/// A union source relates when every member does, a union target when some
/// member takes the source; literals, `never`, `unknown`, `any`, `void` and the
/// nullish types relate by the checker's rules.
#[test]
fn union_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(UNIONS);
    let mut failures = matrix.types(&[
        ("[AB] extends [ABC] ? 1 : 2", "1"),
        ("[ABC] extends [AB] ? 1 : 2", "2"),
        ("[\"a\"] extends [AB] ? 1 : 2", "1"),
        ("[string] extends [AB] ? 1 : 2", "2"),
        ("[AB] extends [string] ? 1 : 2", "1"),
        ("[Cat] extends [Pet] ? 1 : 2", "1"),
        ("[Pet] extends [Cat] ? 1 : 2", "2"),
        (
            "[{ kind: \"circle\" | \"square\"; r: number }] extends [Shape] ? 1 : 2",
            "2",
        ),
        (
            "[string | number] extends [string | number | boolean] ? 1 : 2",
            "1",
        ),
        ("[string | number] extends [string] ? 1 : 2", "2"),
        ("[never] extends [string] ? 1 : 2", "1"),
        ("[string] extends [never] ? 1 : 2", "2"),
        ("[{}] extends [unknown] ? 1 : 2", "1"),
        ("[any] extends [string] ? 1 : 2", "1"),
        ("[string] extends [any] ? 1 : 2", "1"),
        ("[1 | 2 | 3] extends [number] ? 1 : 2", "1"),
        ("[number] extends [1 | 2 | 3] ? 1 : 2", "2"),
        ("[`a${string}`] extends [string] ? 1 : 2", "1"),
        (
            "[\"a\" | 1 | true] extends [string | number | boolean] ? 1 : 2",
            "1",
        ),
        ("[undefined] extends [void] ? 1 : 2", "1"),
        ("[void] extends [undefined] ? 1 : 2", "2"),
        ("[undefined] extends [string | void] ? 1 : 2", "1"),
        ("[symbol] extends [string | number | symbol] ? 1 : 2", "1"),
        ("[bigint] extends [number] ? 1 : 2", "2"),
        ("[1n] extends [bigint] ? 1 : 2", "1"),
        ("[-1] extends [number] ? 1 : 2", "1"),
    ]);
    failures.extend(matrix.nullness(&[
        (
            Read::Type("[string | undefined] extends [string] ? 1 : 2"),
            "2",
            "1",
        ),
        (
            Read::Type("[string | null] extends [string] ? 1 : 2"),
            "2",
            "1",
        ),
        (Read::Type("[null] extends [void] ? 1 : 2"), "2", "1"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An object source whose discriminant property is a union relates to a target
/// union by splitting on the discriminant (`typeRelatedToDiscriminatedType`):
/// each combination of discriminant values finds a target member.
/// Wrong-but-clean.
///
/// What the lane gives:
/// - `[{ kind: "cat" | "dog" }] extends [{ kind: "cat" } | { kind: "dog" }] ? 1
///   : 2`: the checker answers `1`; the lane measured `2`.
/// - `[{ kind: "circle" | "square"; r: number; s: number }] extends [Shape] ? 1
///   : 2`: the checker answers `1`; the lane measured `2`.
/// - `[{ a: 1 | 2 }] extends [{ a: 1 } | { a: 2 }] ? 1 : 2`: the checker
///   answers `1`; the lane measured `2`.
/// - `[{ a: 1 | 2; b: string }] extends [{ a: 1; b: string } | { a: 2; b:
///   string }] ? 1 : 2`: the checker answers `1`; the lane measured `2`.
/// - `[{ a: boolean }] extends [{ a: true } | { a: false }] ? 1 : 2`: the
///   checker answers `1`; the lane measured `2`.
#[test]
#[ignore = "an object source with a union discriminant relates to a discriminated target union"]
fn wrong_clean_a_discriminated_object_source_relates_to_a_target_union() {
    let matrix = Matrix::new(UNIONS);
    let failures = matrix.types(&[
        (
            "[{ kind: \"cat\" | \"dog\" }] extends [{ kind: \"cat\" } | { kind: \"dog\" }] ? 1 : 2",
            "1",
        ),
        (
            "[{ kind: \"circle\" | \"square\"; r: number; s: number }] extends [Shape] ? 1 : 2",
            "1",
        ),
        ("[{ a: 1 | 2 }] extends [{ a: 1 } | { a: 2 }] ? 1 : 2", "1"),
        (
            "[{ a: 1 | 2; b: string }] extends [{ a: 1; b: string } | { a: 2; b: string }] ? 1 : 2",
            "1",
        ),
        (
            "[{ a: boolean }] extends [{ a: true } | { a: false }] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `boolean` IS `true | false`, so each is below the other.
#[test]
fn boolean_relates_as_the_checker_relates_true_or_false() {
    let matrix = Matrix::new(UNIONS);
    let failures = matrix.types(&[("[boolean] extends [true | false] ? 1 : 2", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `unknown` is below `{} | null | undefined` in every setting, and below `{}`
/// alone when `strictNullChecks` is off. Wrong-but-clean.
///
/// What the lane gives:
/// - `[unknown] extends [{}] ? 1 : 2`: the checker answers `2` (strict), `1`
///   (strictNullChecks off), `2` (noImplicitAny off), `1` (both off); the lane
///   measured `2` (strictNullChecks off, both off).
/// - `[unknown] extends [{} | null | undefined] ? 1 : 2`: the checker answers
///   `1`; the lane measured `2`.
#[test]
#[ignore = "unknown relates to {} | null | undefined, and to {} without strictNullChecks"]
fn wrong_clean_unknown_is_below_empty_object_and_nullish() {
    let matrix = Matrix::new(UNIONS);
    let mut failures = matrix.types(&[("[unknown] extends [{} | null | undefined] ? 1 : 2", "1")]);
    failures.extend(matrix.nullness(&[(Read::Type("[unknown] extends [{}] ? 1 : 2"), "2", "1")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Interfaces and a branded primitive.
const INTERSECTIONS: &str = r##"
interface A { a: string }
interface B { b: number }
type AB = A & B;
type Brand = string & { __brand: "b" };
"##;

/// An intersection source relates when some member does, an intersection target
/// when every member takes the source; disjoint primitives, `unknown`, `any`
/// and `never` reduce as the checker reduces them.
#[test]
fn intersection_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(INTERSECTIONS);
    let failures = matrix.types(&[
        ("[AB] extends [A] ? 1 : 2", "1"),
        ("[AB] extends [B] ? 1 : 2", "1"),
        ("[A] extends [AB] ? 1 : 2", "2"),
        ("[{ a: string; b: number }] extends [AB] ? 1 : 2", "1"),
        ("[AB] extends [{ a: string; b: number }] ? 1 : 2", "1"),
        ("[Brand] extends [string] ? 1 : 2", "1"),
        ("[string] extends [Brand] ? 1 : 2", "2"),
        ("[string & number] extends [never] ? 1 : 2", "1"),
        ("[never] extends [string & number] ? 1 : 2", "1"),
        ("[\"a\" & string] extends [\"a\"] ? 1 : 2", "1"),
        ("[A & { a: \"x\" }] extends [{ a: \"x\" }] ? 1 : 2", "1"),
        (
            "[(A & { c: 1 }) | (B & { c: 1 })] extends [(A | B) & { c: 1 }] ? 1 : 2",
            "1",
        ),
        ("[A & unknown] extends [A] ? 1 : 2", "1"),
        ("[A & any] extends [number] ? 1 : 2", "1"),
        ("[A & never] extends [number] ? 1 : 2", "1"),
        ("[(() => 1) & (() => 2)] extends [() => 1] ? 1 : 2", "1"),
        (
            "[((a: string) => 1) & ((a: number) => 2)] extends [(a: number) => 2] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `(A | B) & { c: 1 }` is the union `(A & { c: 1 }) | (B & { c: 1 })`, so it
/// relates to it. Wrong-but-clean.
///
/// What the lane gives:
/// - `[(A | B) & { c: 1 }] extends [(A & { c: 1 }) | (B & { c: 1 })] ? 1 : 2`:
///   the checker answers `1`; the lane measured `2`.
#[test]
#[ignore = "an intersection with a union member relates as its distributed union"]
fn wrong_clean_an_intersection_over_a_union_relates_as_its_distribution() {
    let matrix = Matrix::new(INTERSECTIONS);
    let failures = matrix.types(&[(
        "[(A | B) & { c: 1 }] extends [(A & { c: 1 }) | (B & { c: 1 })] ? 1 : 2",
        "1",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An intersection of object types whose same-named property has disjoint
/// types, one of them a literal, reduces to `never`; a property whose types
/// overlap, or that holds no literal, keeps the intersection (TypeScript
/// 7.0.2, all four settings alike).
#[test]
fn disjoint_discriminants_reduce_as_the_checker_reduces_them() {
    let matrix = Matrix::new(INTERSECTIONS);
    let failures = matrix.types(&[
        ("[{ a: 1 } & { a: string }] extends [never] ? 1 : 2", "1"),
        ("[{ a: 1 } & { a: number }] extends [never] ? 1 : 2", "2"),
        (
            "[{ a: string } & { a: number }] extends [never] ? 1 : 2",
            "2",
        ),
        ("[{ a: 1; b: 2 } & { a: 1 }] extends [never] ? 1 : 2", "2"),
        ("[{ a: 1 } & { a: 2 }] extends [never] ? 1 : 2", "1"),
        (
            "[{ k: \"x\"; v: 1 } & { k: \"y\" }] extends [never] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Classes with private, protected, ECMAScript-private and static members.
const PRIVATE_MEMBERS: &str = r##"
class C1 { private p = 1; q = 2 }
class C2 { private p = 1; q = 2 }
class C3 { protected p = 1; q = 2 }
class D1 extends C1 { r = 3 }
class Pub { p = 1; q = 2 }
class E1 { #h = 1; q = 2 }
class E2 { #h = 1; q = 2 }
class St { static s = 1; i = 2 }
"##;

/// A `private` or `protected` member relates only to the same declaration, so
/// two structurally equal classes with private members are unrelated, a public
/// twin fits neither way, and a subclass fits its base; a class's constructor
/// type carries its statics.
#[test]
fn private_and_protected_members_relate_nominally() {
    let matrix = Matrix::new(PRIVATE_MEMBERS);
    let failures = matrix.types(&[
        ("[C1] extends [C2] ? 1 : 2", "2"),
        ("[C1] extends [C1] ? 1 : 2", "1"),
        ("[D1] extends [C1] ? 1 : 2", "1"),
        ("[C1] extends [D1] ? 1 : 2", "2"),
        ("[Pub] extends [C1] ? 1 : 2", "2"),
        ("[C1] extends [Pub] ? 1 : 2", "2"),
        ("[C1] extends [{ q: number }] ? 1 : 2", "1"),
        ("[C3] extends [C1] ? 1 : 2", "2"),
        ("[{ q: number }] extends [C1] ? 1 : 2", "2"),
        ("[E1] extends [{ q: number }] ? 1 : 2", "1"),
        ("[St] extends [{ i: number }] ? 1 : 2", "1"),
        ("[typeof St] extends [{ s: number }] ? 1 : 2", "1"),
        ("[typeof St] extends [new () => St] ? 1 : 2", "1"),
        ("[typeof D1] extends [typeof C1] ? 1 : 2", "1"),
        ("[typeof C1] extends [typeof D1] ? 1 : 2", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An ECMAScript private name `#h` is unique to its class, so two classes that
/// each declare one are unrelated. Wrong-but-clean.
///
/// What the lane gives:
/// - `[E1] extends [E2] ? 1 : 2`: the checker answers `2`; the lane measured
///   `1`.
#[test]
#[ignore = "a #private member relates only to its own declaration"]
fn wrong_clean_an_ecmascript_private_name_relates_nominally() {
    let matrix = Matrix::new(PRIVATE_MEMBERS);
    let failures = matrix.types(&[("[E1] extends [E2] ? 1 : 2", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic interfaces with and without variance annotations, and a generic
/// function.
const GENERICS: &str = r##"
interface Box<T> { v: T }
interface ROBox<out T> { readonly v: T }
interface Sink<in T> { put(v: T): void }
interface Fnb<T> { (v: T): void }
type Id<T> = T;
type Pair<A, B> = [A, B];
declare function idf<T>(x: T): T;
"##;

/// Generic applications relate by their arguments' variance: covariant through
/// a property, contravariant through a callback parameter, as annotated with
/// `in` / `out`; aliases and tuples relate by what they apply to.
#[test]
fn generic_applications_relate_by_their_arguments() {
    let matrix = Matrix::new(GENERICS);
    let failures = matrix.types(&[
        ("[Box<string>] extends [Box<string | number>] ? 1 : 2", "1"),
        ("[Box<string | number>] extends [Box<string>] ? 1 : 2", "2"),
        (
            "[ROBox<string>] extends [ROBox<string | number>] ? 1 : 2",
            "1",
        ),
        (
            "[ROBox<string | number>] extends [ROBox<string>] ? 1 : 2",
            "2",
        ),
        (
            "[Sink<string | number>] extends [Sink<string>] ? 1 : 2",
            "1",
        ),
        (
            "[Sink<string>] extends [Sink<string | number>] ? 1 : 2",
            "2",
        ),
        ("[Fnb<string | number>] extends [Fnb<string>] ? 1 : 2", "1"),
        ("[Fnb<string>] extends [Fnb<string | number>] ? 1 : 2", "2"),
        ("[Id<string>] extends [string] ? 1 : 2", "1"),
        ("[Pair<1, 2>] extends [[number, number]] ? 1 : 2", "1"),
        ("[Box<never>] extends [Box<string>] ? 1 : 2", "1"),
        ("[Box<any>] extends [Box<string>] ? 1 : 2", "1"),
        ("[Box<unknown>] extends [Box<string>] ? 1 : 2", "2"),
        ("[Box<Box<1>>] extends [Box<Box<number>>] ? 1 : 2", "1"),
        ("[Array<string>] extends [string[]] ? 1 : 2", "1"),
        (
            "[ReadonlyArray<string>] extends [readonly string[]] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A generic source signature is instantiated in the context of the target
/// signature (`instantiateSignatureInContextOf`) before its parameters and
/// return relate; a non-generic source is not below a generic target.
///
/// What the lane gives:
/// - `[typeof idf] extends [(x: string) => string] ? 1 : 2`: the checker
///   answers `1`; the lane measured `<unreduced conditional>`.
/// - `[typeof idf] extends [<U>(x: U) => U] ? 1 : 2`: the checker answers `1`;
///   the lane measured `<unreduced conditional>`.
/// - `[(x: string) => string] extends [typeof idf] ? 1 : 2`: the checker
///   answers `2`; the lane measured `<unreduced conditional>`.
/// - `[<T>(x: T) => T[]] extends [(x: number) => number[]] ? 1 : 2`: the
///   checker answers `1`; the lane measured `<unreduced conditional>`.
/// - `[<T>(x: T) => T[]] extends [(x: number) => string[]] ? 1 : 2`: the
///   checker answers `2`; the lane measured `<unreduced conditional>`.
/// - `[<T extends string>(x: T) => T] extends [(x: number) => number] ? 1 : 2`:
///   the checker answers `2`; the lane measured `<unreduced conditional>`.
#[test]
#[ignore = "a generic signature relates to a signature by instantiating its type parameters"]
fn a_generic_signature_relates_by_instantiating_it() {
    let matrix = Matrix::new(GENERICS);
    let failures = matrix.types(&[
        ("[typeof idf] extends [(x: string) => string] ? 1 : 2", "1"),
        ("[typeof idf] extends [<U>(x: U) => U] ? 1 : 2", "1"),
        ("[(x: string) => string] extends [typeof idf] ? 1 : 2", "2"),
        (
            "[<T>(x: T) => T[]] extends [(x: number) => number[]] ? 1 : 2",
            "1",
        ),
        (
            "[<T>(x: T) => T[]] extends [(x: number) => string[]] ? 1 : 2",
            "2",
        ),
        (
            "[<T extends string>(x: T) => T] extends [(x: number) => number] ? 1 : 2",
            "2",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Numeric, string, const and heterogeneous enums.
const ENUMS: &str = r##"
enum E { A, B }
enum S { X = "x", Y = "y" }
const enum CE { P = 1, Q = 2 }
enum Mixed { N = 1, T = "t" }
"##;

/// Enum members relate to their enum and their literal values (a number to a
/// numeric enum, never a string to a string enum); a template literal relates
/// by the strings it can produce, and a string to a `${number}` hole by its
/// numeric spelling.
#[test]
fn enum_and_template_literal_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(ENUMS);
    let failures = matrix.types(&[
        ("[E.A] extends [E] ? 1 : 2", "1"),
        ("[E] extends [E.A] ? 1 : 2", "2"),
        ("[E.A] extends [number] ? 1 : 2", "1"),
        ("[number] extends [E] ? 1 : 2", "1"),
        ("[0] extends [E] ? 1 : 2", "1"),
        ("[5] extends [E] ? 1 : 2", "2"),
        ("[E] extends [number] ? 1 : 2", "1"),
        ("[S.X] extends [string] ? 1 : 2", "1"),
        ("[\"x\"] extends [S] ? 1 : 2", "2"),
        ("[S.X] extends [\"x\"] ? 1 : 2", "1"),
        ("[S] extends [string] ? 1 : 2", "1"),
        ("[S] extends [\"x\" | \"y\"] ? 1 : 2", "1"),
        ("[CE.P] extends [1] ? 1 : 2", "1"),
        ("[CE] extends [1 | 2] ? 1 : 2", "1"),
        ("[Mixed.N] extends [number] ? 1 : 2", "1"),
        ("[Mixed] extends [string | number] ? 1 : 2", "1"),
        ("[`${E.A}`] extends [\"0\"] ? 1 : 2", "1"),
        ("[`${S.X}`] extends [\"x\"] ? 1 : 2", "1"),
        ("[`${number}`] extends [string] ? 1 : 2", "1"),
        ("[\"1.5\"] extends [`${number}`] ? 1 : 2", "1"),
        ("[\"-0\"] extends [`${number}`] ? 1 : 2", "1"),
        ("[\"0x10\"] extends [`${number}`] ? 1 : 2", "1"),
        ("[\"\"] extends [`${number}`] ? 1 : 2", "2"),
        ("[\"a\"] extends [`${string}`] ? 1 : 2", "1"),
        ("[`${boolean}`] extends [\"true\" | \"false\"] ? 1 : 2", "1"),
        ("[\"true\" | \"false\"] extends [`${boolean}`] ? 1 : 2", "1"),
        ("[`${1 | 2}`] extends [\"1\" | \"2\"] ? 1 : 2", "1"),
        (
            "[`a${\"b\" | \"c\"}`] extends [\"ab\" | \"ac\"] ? 1 : 2",
            "1",
        ),
        (
            "[\"prefix-mid-suffix\"] extends [`prefix-${string}-suffix`] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A literal is below `Lowercase<string>` / `Capitalize<string>` /
/// `Uncapitalize<string>` when the mapping leaves it unchanged, and a template
/// over a mapping is below `string`; the operands here are one-element tuples.
///
/// What the lane gives:
/// - `[`${Lowercase<string>}`] extends [string] ? 1 : 2`: the checker answers
///   `1`; the lane measured `<unreduced conditional>`.
/// - `["abc"] extends [Lowercase<string>] ? 1 : 2`: the checker answers `1`;
///   the lane measured `<unreduced conditional>`.
/// - `["aBc"] extends [Lowercase<string>] ? 1 : 2`: the checker answers `2`;
///   the lane measured `<unreduced conditional>`.
/// - `["Abc"] extends [Capitalize<string>] ? 1 : 2`: the checker answers `1`;
///   the lane measured `<unreduced conditional>`.
/// - `["abc"] extends [Capitalize<string>] ? 1 : 2`: the checker answers `2`;
///   the lane measured `<unreduced conditional>`.
/// - `["ab"] extends [Uncapitalize<string>] ? 1 : 2`: the checker answers `1`;
///   the lane measured `<unreduced conditional>`.
#[test]
#[ignore = "Lowercase, Capitalize and Uncapitalize over string relate by the mapping"]
fn a_string_mapping_of_string_relates_by_the_mapping_in_a_tuple() {
    let matrix = Matrix::new(ENUMS);
    let failures = matrix.types(&[
        ("[`${Lowercase<string>}`] extends [string] ? 1 : 2", "1"),
        ("[\"abc\"] extends [Lowercase<string>] ? 1 : 2", "1"),
        ("[\"aBc\"] extends [Lowercase<string>] ? 1 : 2", "2"),
        ("[\"Abc\"] extends [Capitalize<string>] ? 1 : 2", "1"),
        ("[\"abc\"] extends [Capitalize<string>] ? 1 : 2", "2"),
        ("[\"ab\"] extends [Uncapitalize<string>] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An interface the library-backed rows relate to.
const LIB_RELATIONS: &str = r##"
interface HasLen { length: number }
"##;

/// A minimal library, registered as each project's lib and passed to the
/// checker as its only lib file (`--noLib`).
const RELATION_LIB: &str = r##"interface Array<T> { length: number; [n: number]: T; push(...items: T[]): number; }
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
interface Boolean { valueOf(): boolean; }
interface Function { apply(this: Function, thisArg: any, argArray?: any): any; }
interface IArguments {}
interface Number { toFixed(fractionDigits?: number): string; }
interface Object { toString(): string; }
interface RegExp {}
interface CallableFunction {}
interface NewableFunction {}
interface String { readonly length: number; charAt(pos: number): string; readonly [index: number]: string; }
declare type PropertyKey = string | number | symbol;
"##;

/// Rows over a registered library: a global alias, a wrapper interface below
/// its primitive (never the reverse), a readonly array missing a mutating
/// method, and a tuple below a readonly array.
#[test]
fn library_wrapper_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(LIB_RELATIONS).lib(RELATION_LIB);
    let failures = matrix.types(&[
        ("[string[]] extends [{ [n: number]: number }] ? 1 : 2", "2"),
        (
            "[readonly string[]] extends [{ push(...items: string[]): number }] ? 1 : 2",
            "2",
        ),
        (
            "[PropertyKey] extends [string | number | symbol] ? 1 : 2",
            "1",
        ),
        ("[\"k\"] extends [PropertyKey] ? 1 : 2", "1"),
        ("[Object] extends [{}] ? 1 : 2", "1"),
        ("[string] extends [{ length: string }] ? 1 : 2", "2"),
        (
            "[number] extends [{ toFixed(d: string): string }] ? 1 : 2",
            "2",
        ),
        ("[String] extends [string] ? 1 : 2", "2"),
        ("[[1, 2]] extends [readonly number[]] ? 1 : 2", "1"),
        ("[{ length: number }] extends [string] ? 1 : 2", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A primitive source relates to an object target through its apparent type,
/// the library's wrapper interface (`String`, `Number`, `Boolean`): `string`
/// has `length`, `charAt` and a number index, `number` has `toFixed`, `true` is
/// below `Boolean`.
#[test]
fn a_primitive_relates_as_the_checker_relates_its_wrapper_interface() {
    let matrix = Matrix::new(LIB_RELATIONS).lib(RELATION_LIB);
    let failures = matrix.types(&[
        ("[string] extends [HasLen] ? 1 : 2", "1"),
        (
            "[string] extends [{ charAt(p: number): string }] ? 1 : 2",
            "1",
        ),
        ("[string] extends [{ [n: number]: string }] ? 1 : 2", "1"),
        ("[number] extends [{ toFixed(): string }] ? 1 : 2", "1"),
        ("[boolean] extends [{ valueOf(): boolean }] ? 1 : 2", "1"),
        ("[true] extends [Boolean] ? 1 : 2", "1"),
        ("[\"s\"] extends [String] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An array or tuple source relates to an object target through the library's
/// `Array<T>` interface: its number index, `length` and `push`.
#[test]
fn an_array_relates_as_the_checker_relates_the_array_interface() {
    let matrix = Matrix::new(LIB_RELATIONS).lib(RELATION_LIB);
    let failures = matrix.types(&[
        ("[string[]] extends [{ [n: number]: string }] ? 1 : 2", "1"),
        ("[string[]] extends [{ length: number }] ? 1 : 2", "1"),
        (
            "[string[]] extends [{ push(...items: string[]): number }] ? 1 : 2",
            "1",
        ),
        ("[[1, 2]] extends [HasLen] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{}` is below the global `Object` interface: every object type's apparent
/// type carries `Object`'s members.
#[test]
fn the_empty_object_type_relates_to_object_as_the_checker_relates_it() {
    let matrix = Matrix::new(LIB_RELATIONS).lib(RELATION_LIB);
    let failures = matrix.types(&[("[{}] extends [Object] ? 1 : 2", "1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under `strictBindCallApply` (part of `strict`) a function type's apparent
/// type is `CallableFunction`, not `Function`; with a library whose
/// `CallableFunction` declares no `apply`, a function type does not have one.
/// Wrong-but-clean.
///
/// What the lane gives:
/// - `[() => void] extends [{ apply(this: Function, thisArg: any, argArray?:
///   any): any }] ? 1 : 2`: the checker answers `2`; the lane measured `1`.
#[test]
#[ignore = "a function type's apparent type is CallableFunction under strictBindCallApply"]
fn wrong_clean_a_function_type_relates_through_callable_function() {
    let matrix = Matrix::new(LIB_RELATIONS).lib(RELATION_LIB);
    let failures = matrix.types(&[
        ("[() => void] extends [{ apply(this: Function, thisArg: any, argArray?: any): any }] ? 1 : 2", "2"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Callbacks, an overloaded interface and an abstract class.
const SIGNATURES: &str = r##"
interface Animal { name: string }
interface Dog2 extends Animal { bark(): void }
interface Over { (a: string): string; (a: number): number }
type Cb = (e: Animal) => void;
type CbDog = (e: Dog2) => void;
abstract class Abs { abstract m(): void }
class Conc extends Abs { m() {} }
"##;

/// Signatures relate contravariantly in their parameters under
/// `strictFunctionTypes`, covariantly in their return, by arity and rest
/// elements, by overload (every target signature finds a source signature), by
/// construct-signature abstractness, and by type predicate.
#[test]
fn signatures_relate_by_parameters_and_return() {
    let matrix = Matrix::new(SIGNATURES);
    let failures = matrix.types(&[
        ("[CbDog] extends [Cb] ? 1 : 2", "2"),
        ("[Cb] extends [CbDog] ? 1 : 2", "1"),
        ("[(a?: string) => void] extends [(a: string) => void] ? 1 : 2", "1"),
        ("[(...a: string[]) => void] extends [(a: string, b: string) => void] ? 1 : 2", "1"),
        ("[(a: string, b: string) => void] extends [(...a: string[]) => void] ? 1 : 2", "1"),
        ("[(a: string, ...r: number[]) => void] extends [(a: string, b: number) => void] ? 1 : 2", "1"),
        ("[(...a: [string, number]) => void] extends [(a: string, b: number) => void] ? 1 : 2", "1"),
        ("[(a: string, b: number) => void] extends [(...a: [string, number]) => void] ? 1 : 2", "1"),
        ("[() => void] extends [() => undefined] ? 1 : 2", "2"),
        ("[() => undefined] extends [() => void] ? 1 : 2", "1"),
        ("[() => string] extends [() => void] ? 1 : 2", "1"),
        ("[Over] extends [(a: string) => string] ? 1 : 2", "1"),
        ("[Over] extends [(a: number) => number] ? 1 : 2", "1"),
        ("[Over] extends [(a: boolean) => boolean] ? 1 : 2", "2"),
        ("[(a: string) => string] extends [Over] ? 1 : 2", "2"),
        ("[typeof Conc] extends [typeof Abs] ? 1 : 2", "1"),
        ("[typeof Abs] extends [typeof Conc] ? 1 : 2", "2"),
        ("[typeof Conc] extends [abstract new () => Abs] ? 1 : 2", "1"),
        ("[typeof Abs] extends [new () => Abs] ? 1 : 2", "2"),
        ("[typeof Abs] extends [abstract new () => Abs] ? 1 : 2", "1"),
        ("[(this: Animal) => void] extends [() => void] ? 1 : 2", "1"),
        ("[(x: Animal) => x is Dog2] extends [(x: Animal) => boolean] ? 1 : 2", "1"),
        ("[(x: Animal) => boolean] extends [(x: Animal) => x is Dog2] ? 1 : 2", "2"),
        ("[(x: Animal) => x is Dog2] extends [(x: Animal) => x is Animal] ? 1 : 2", "1"),
        ("[(x: Animal) => x is Animal] extends [(x: Animal) => x is Dog2] ? 1 : 2", "2"),
        ("[(x: unknown) => asserts x is string] extends [(x: unknown) => void] ? 1 : 2", "1"),
        ("[new () => Dog2] extends [new () => Animal] ? 1 : 2", "1"),
        ("[new () => Animal] extends [new () => Dog2] ? 1 : 2", "2"),
        ("[{ (): void; p: 1 }] extends [() => void] ? 1 : 2", "1"),
        ("[{ m(x: Animal): void }] extends [{ m(x: Dog2): void }] ? 1 : 2", "1"),
        ("[{ f: (x: Dog2) => void }] extends [{ f: (x: Animal) => void }] ? 1 : 2", "2"),
        ("[{ f: (x: Animal) => void }] extends [{ f: (x: Dog2) => void }] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under `strictNullChecks` an optional parameter's type includes `undefined`,
/// so a source whose same parameter is a required `string` does not fit `(a?:
/// string) => void` (`2`, and `1` with `strictNullChecks` off), while an
/// optional source parameter fits a required one and `(a: string | undefined)
/// => void` and `(a?: string) => void` fit each other (`1` under every
/// setting, measured on TypeScript 7.0.2).
#[test]
fn an_optional_parameter_relates_as_the_checker_relates_its_undefined() {
    let matrix = Matrix::new(SIGNATURES);
    let mut failures = matrix.nullness(&[(
        Read::Type("[(a: string) => void] extends [(a?: string) => void] ? 1 : 2"),
        "2",
        "1",
    )]);
    failures.extend(matrix.types(&[
        (
            "[(a?: string) => void] extends [(a: string) => void] ? 1 : 2",
            "1",
        ),
        (
            "[(a: string | undefined) => void] extends [(a?: string) => void] ? 1 : 2",
            "1",
        ),
        (
            "[(a?: string) => void] extends [(a: string | undefined) => void] ? 1 : 2",
            "1",
        ),
        ("[(a?: string) => void] extends [() => void] ? 1 : 2", "1"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Two method members compare their parameters bivariantly even under
/// `strictFunctionTypes`, so a method taking `Dog2` fits one taking `Animal`.
/// Wrong-but-clean.
///
/// What the lane gives:
/// - `[{ m(x: Dog2): void }] extends [{ m(x: Animal): void }] ? 1 : 2`: the
///   checker answers `1`; the lane measured `2`.
#[test]
#[ignore = "method members compare their parameters bivariantly"]
fn wrong_clean_a_method_member_relates_its_parameters_bivariantly() {
    let matrix = Matrix::new(SIGNATURES);
    let failures = matrix.types(&[(
        "[{ m(x: Dog2): void }] extends [{ m(x: Animal): void }] ? 1 : 2",
        "1",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Recursive interfaces and aliases, a unique symbol key, a weak type and a
/// numeric key.
const RECURSIVE_AND_WEAK: &str = r##"
interface Tree { v: number; kids: Tree[] }
interface Tree2 { v: number; kids: Tree2[] }
interface LinkS { v: string; next?: LinkS }
type Json = string | number | boolean | null | Json[] | { [k: string]: Json };
declare const sym: unique symbol;
interface WithSym { [sym]: 1 }
interface Weak { a?: number; b?: string }
interface N1 { 1: "one" }
"##;

/// Recursive types relate structurally, a unique-symbol or numeric-named
/// property relates by its key, an optional member takes a missing property,
/// and variadic tuples relate element by element.
#[test]
fn recursive_weak_and_keyed_types_relate_as_the_checker_relates_them() {
    let matrix = Matrix::new(RECURSIVE_AND_WEAK);
    let failures = matrix.types(&[
        ("[Tree] extends [Tree2] ? 1 : 2", "1"),
        ("[Tree2] extends [Tree] ? 1 : 2", "1"),
        ("[{ v: number; kids: [] }] extends [Tree] ? 1 : 2", "1"),
        ("[{ v: \"s\" }] extends [LinkS] ? 1 : 2", "1"),
        (
            "[{ v: \"s\"; next: { v: 1 } }] extends [LinkS] ? 1 : 2",
            "2",
        ),
        ("[{ a: [1, \"x\"] }] extends [Json] ? 1 : 2", "1"),
        ("[WithSym] extends [{ [sym]: number }] ? 1 : 2", "1"),
        ("[{ [sym]: 2 }] extends [WithSym] ? 1 : 2", "2"),
        ("[{ a: 1; c: 1 }] extends [Weak] ? 1 : 2", "1"),
        ("[{}] extends [Weak] ? 1 : 2", "1"),
        ("[string] extends [Weak] ? 1 : 2", "2"),
        ("[N1] extends [{ \"1\": string }] ? 1 : 2", "1"),
        ("[{ \"1\": \"one\" }] extends [N1] ? 1 : 2", "1"),
        ("[{ a: never }] extends [{ a: string }] ? 1 : 2", "1"),
        ("[{ a: string }] extends [{ a: never }] ? 1 : 2", "2"),
        (
            "[{ a?: string }] extends [{ a: string | undefined }] ? 1 : 2",
            "2",
        ),
        ("[{ a?: undefined }] extends [{}] ? 1 : 2", "1"),
        ("[object] extends [{}] ? 1 : 2", "1"),
        ("[{}] extends [object] ? 1 : 2", "1"),
        ("[{ toString(): string }] extends [{}] ? 1 : 2", "1"),
        (
            "[[string, ...number[]]] extends [[string, number, ...number[]]] ? 1 : 2",
            "2",
        ),
        (
            "[[...string[], number]] extends [[...string[], number]] ? 1 : 2",
            "1",
        ),
        (
            "[[1, ...string[]]] extends [[number, ...(string | number)[]]] ? 1 : 2",
            "1",
        ),
        (
            "[readonly [a: 1, b?: 2]] extends [readonly [number, number?]] ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `{ a: () => void }` is not below `Json` (`string | number | boolean | null |
/// Json[] | { [k: string]: Json }`): the function fits no arm of the index
/// signature's value. Wrong-but-clean.
///
/// What the lane gives:
/// - `[{ a: () => void }] extends [Json] ? 1 : 2`: the checker answers `2`; the
///   lane measured `1`.
#[test]
#[ignore = "a function-valued property fits no arm of a recursive JSON alias"]
fn wrong_clean_a_function_member_is_not_below_a_recursive_json_alias() {
    let matrix = Matrix::new(RECURSIVE_AND_WEAK);
    let failures = matrix.types(&[("[{ a: () => void }] extends [Json] ? 1 : 2", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A WEAK target (every property optional) takes a source only when they share
/// a property, so `{ c: 1 }` is not below `{ a?: number; b?: string }`.
/// Wrong-but-clean.
///
/// What the lane gives:
/// - `[{ c: 1 }] extends [Weak] ? 1 : 2`: the checker answers `2`; the lane
///   measured `1`.
#[test]
#[ignore = "a source sharing no property with a weak target is not assignable"]
fn wrong_clean_a_weak_target_needs_a_shared_property() {
    let matrix = Matrix::new(RECURSIVE_AND_WEAK);
    let failures = matrix.types(&[("[{ c: 1 }] extends [Weak] ? 1 : 2", "2")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under `strictNullChecks` `{ a: string | undefined }` fits `{ a?: string }`:
/// the optional target's read type includes `undefined`.
#[test]
fn an_optional_property_relates_as_the_checker_relates_its_undefined() {
    let matrix = Matrix::new(RECURSIVE_AND_WEAK);
    let failures = matrix.types(&[(
        "[{ a: string | undefined }] extends [{ a?: string }] ? 1 : 2",
        "1",
    )]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
