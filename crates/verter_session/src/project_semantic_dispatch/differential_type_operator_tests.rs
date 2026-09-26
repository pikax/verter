//! Differential probes of the checker's type operators: indexed access,
//! `keyof`, mapped types with modifiers and key remapping, conditional
//! types with `infer`, distribution and recursion, the library utility
//! types (`ReturnType`, `Parameters`, `Exclude`, `Extract`,
//! `Awaited`, `Record`, `Pick`, `Omit`, …), template literal types and
//! the intrinsic string mappings. Each row is a type in TYPE position over
//! the fixture.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact probe
//! with `tsc --ignoreConfig --declaration --emitDeclarationOnly --strict
//! --noErrorTruncation` under each `strictNullChecks` × `noImplicitAny`
//! setting: `declare const p: <probe>; const s: never = p;` read off the
//! TS2322 message. A row with one answer answers alike in the four settings;
//! a row with two answers gives the `strictNullChecks` answer then the
//! answer with it off. An ignored test asserts the measured answer for rows
//! the lane does not yet answer as the checker does; "wrong-but-clean"
//! marks a lane answer published complete and undegraded, and
//! "unreduced" one that denotes the checker's type but keeps an operator
//! the checker's print has resolved.

use super::differential_harness_tests::{Matrix, Read};

/// An interface, a tuple, an array alias, a nested object and a discriminated
/// union.
const INDEXED: &str = r##"
interface User { id: number; name: string; tags: string[]; meta?: { a: 1 }; readonly r: boolean }
type Tup = [string, number, boolean];
type Arr = Array<{ x: 1 }>;
type Nested = { a: { b: { c: "deep" } } };
type U2 = { k: "a"; v: 1 } | { k: "b"; v: 2 };
type Obj = { a: 1; b: 2 };
type Both = { a: 1 } & { b: 2 };
"##;

/// An indexed access reads a property, a union of keys, every key, a tuple
/// element or its `length`, an array element and a union's shared property;
/// `keyof` over an interface keeps its name, and over an intersection, a union,
/// an index signature, `any`, `never`, `unknown` and `{}` reduces as the
/// checker reduces it.
#[test]
fn indexed_access_and_keyof_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(INDEXED);
    let mut failures = matrix.types(&[
        ("User['id']", "number"),
        ("User['id' | 'name']", "string | number"),
        ("keyof User", "keyof User"),
        ("Tup[0]", "string"),
        ("Tup[number]", "string | number | boolean"),
        ("Tup['length']", "3"),
        ("Arr[number]", "{ x: 1; }"),
        ("Arr[0]", "{ x: 1; }"),
        ("Nested['a']['b']['c']", "\"deep\""),
        ("U2['v']", "1 | 2"),
        ("U2['k']", "\"a\" | \"b\""),
        ("keyof (U2 | { k: 'c' })", "\"k\""),
        ("keyof { [k: string]: 1 }", "string | number"),
        ("keyof { [k: number]: 1 }", "number"),
        ("keyof ({ a: 1 } & { b: 2 })", "\"a\" | \"b\""),
        ("keyof any", "string | number | symbol"),
        ("keyof never", "string | number | symbol"),
        ("keyof unknown", "never"),
        ("keyof {}", "never"),
        ("{ a: 1; b: 2 }[keyof { a: 1 }]", "1"),
        ("User['tags'][number]", "string"),
        (
            "Exclude<keyof User, 'r' | 'meta'>",
            "\"id\" | \"name\" | \"tags\"",
        ),
    ]);
    failures.extend(matrix.nullness(&[
        (
            Read::Type("User['meta']"),
            "{ a: 1; } | undefined",
            "{ a: 1; }",
        ),
        (
            Read::Type("User[keyof User]"),
            "string | number | boolean | string[] | { a: 1; } | undefined",
            "string | number | boolean | string[] | { a: 1; }",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` keeps its origin in print only over a declaration's own key list:
/// `keyof Obj` over `type Obj = { a: 1; b: 2 }` prints `keyof Obj`, while
/// `keyof U2` over a union alias is `"k" | "v"` and `keyof Both` over an
/// intersection alias is `"a" | "b"` (the keys are read off the members), and
/// a union of `keyof` object literal types holds their keys (`keyof { a: 1 } |
/// keyof { b: 2 }` is `"a" | "b"`).
#[test]
fn keyof_an_alias_or_literal_type_prints_as_the_checker_prints_it() {
    let matrix = Matrix::new(INDEXED);
    let failures = matrix.types(&[
        ("keyof U2", "\"k\" | \"v\""),
        ("keyof Obj", "keyof Obj"),
        ("keyof Both", "\"a\" | \"b\""),
        ("keyof { a: 1 } | keyof { b: 2 }", "\"a\" | \"b\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Homomorphic, modifier-changing, key-remapping and filtering mapped types.
const MAPPED: &str = r##"
interface User { id: number; name: string; meta?: { a: 1 }; readonly r: boolean }
type Getters<T> = { [K in keyof T as `get${Capitalize<string & K>}`]: () => T[K] };
type Flags<T> = { [K in keyof T]: boolean };
type NoRO<T> = { -readonly [K in keyof T]: T[K] };
type Req<T> = { [K in keyof T]-?: T[K] };
type Opt<T> = { [K in keyof T]+?: T[K] };
type FilterStr<T> = { [K in keyof T as T[K] extends string ? K : never]: T[K] };
type FromUnion<K extends string> = { [P in K]: P };
type Boxed<T> = { [K in keyof T]: { v: T[K] } };
"##;

/// A mapped type maps each key's value, adds or removes `readonly` and `?`,
/// filters keys through `as … never`, maps a union of literal keys, and the
/// library `Partial`, `Required`, `Readonly`, `Pick`, `Omit` and `Record` read
/// as the checker reads them (named applications printed by name).
#[test]
fn mapped_types_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(MAPPED);
    let mut failures = matrix.types(&[
        ("Flags<User>['id']", "boolean"),
        ("NoRO<User>['r']", "boolean"),
        ("[NoRO<{ readonly x: 1 }>] extends [{ x: 1 }] ? 1 : 2", "1"),
        ("Req<User>['meta']", "{ a: 1; }"),
        ("keyof FilterStr<User>", "\"name\""),
        ("FromUnion<'x' | 'y'>['x']", "\"x\""),
        ("Required<User>['meta']", "{ a: 1; }"),
        ("Readonly<{ a: 1 }>", "Readonly<{ a: 1; }>"),
        ("Pick<User, 'id' | 'name'>", "Pick<User, \"id\" | \"name\">"),
        (
            "Omit<User, 'meta' | 'r' | 'name'>",
            "Omit<User, \"meta\" | \"name\" | \"r\">",
        ),
        ("Record<'a' | 'b', number>", "Record<\"a\" | \"b\", number>"),
        ("Record<string, 1>['anything']", "1"),
        ("keyof Record<'a' | 'b', number>", "\"a\" | \"b\""),
        ("Readonly<string[]>", "readonly string[]"),
    ]);
    failures.extend(matrix.nullness(&[
        (
            Read::Type("Partial<User>['id']"),
            "number | undefined",
            "number",
        ),
        (
            Read::Type("Partial<[1, 2]>"),
            "[(1 | undefined)?, (2 | undefined)?]",
            "[1?, 2?]",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Getters<{ a: 1 }>['getA']` with `[K in keyof T as \`get${Capitalize<string
/// & K>}\`]: () => T[K]` is `() => 1`.
///
/// What the lane gives:
/// - `Getters<{ a: 1 }>['getA']`: the checker answers `() => 1`; the lane
///   measured `() => <unreduced indexed access>`.
#[test]
#[ignore = "a mapped type remapping keys through a template reads each remapped member's value"]
fn a_key_remapped_mapped_type_reads_its_remapped_members() {
    let matrix = Matrix::new(MAPPED);
    let failures = matrix.types(&[("Getters<{ a: 1 }>['getA']", "() => 1")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof Flags<User>` prints `keyof User` (a homomorphic mapped type's keys
/// are its source's), `keyof Getters<{ a: 1; b: 2 }>` is `"getA" | "getB"` and
/// `keyof FromUnion<'x' | 'y'>` is `"x" | "y"`: a mapped type's keys are read
/// off its constraint, never as a declaration's key list with a `keyof`
/// origin of its own.
#[test]
fn keyof_a_mapped_application_is_its_key_union() {
    let matrix = Matrix::new(MAPPED);
    let failures = matrix.types(&[
        ("keyof Flags<User>", "keyof User"),
        ("keyof Getters<{ a: 1; b: 2 }>", "\"getA\" | \"getB\""),
        ("keyof FromUnion<'x' | 'y'>", "\"x\" | \"y\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A homomorphic mapped type instantiated with a tuple or array type is a tuple
/// or array, which the alias does not name: `Flags<[1, 2]>` is `[boolean,
/// boolean]`.
#[test]
fn a_homomorphic_mapped_type_maps_arrays_and_tuples() {
    let matrix = Matrix::new(MAPPED);
    let failures = matrix.types(&[("Flags<[1, 2]>", "[boolean, boolean]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Boxed<[1, 2]>` is `[{ v: 1; }, { v: 2; }]` and `Boxed<string[]>` is `{ v:
/// string; }[]`: each element's member value is the source element's type.
///
/// What the lane gives:
/// - `Boxed<[1, 2]>`: the checker answers `[{ v: 1; }, { v: 2; }]`; the lane
///   measured `[{ v: <unreduced indexed access>; }, { v: <unreduced indexed
///   access>; }]`.
/// - `Boxed<string[]>`: the checker answers `{ v: string; }[]`; the lane
///   measured `{ v: <unreduced indexed access>; }[]`.
#[test]
#[ignore = "a homomorphic mapped type over an array or tuple reads each element's member value"]
fn a_homomorphic_mapped_type_over_a_tuple_reads_its_element_values() {
    let matrix = Matrix::new(MAPPED);
    let failures = matrix.types(&[
        ("Boxed<[1, 2]>", "[{ v: 1; }, { v: 2; }]"),
        ("Boxed<string[]>", "{ v: string; }[]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under `strictNullChecks` a mapped type read at a key takes its optionality
/// modifier: `+?` adds `undefined` unless the value holds it (`void` does not
/// stop it), and `-?` removes the `undefined` an optional source member added
/// but not one the member's type spells (`Req<{ a: 1 | undefined }>['a']`
/// stays `1 | undefined`). Without it the value is unchanged. Measured on
/// TypeScript 7.0.2 under all four settings.
#[test]
fn optionality_modifiers_change_the_read_type_as_the_checker_reads_it() {
    let matrix = Matrix::new(MAPPED);
    let mut failures = matrix.types(&[
        ("Required<{ a?: 1 | undefined }>['a']", "1"),
        ("Required<{ a?: 1 }>['a']", "1"),
    ]);
    failures.extend(matrix.nullness(&[
        (Read::Type("Opt<{ a: 1 }>['a']"), "1 | undefined", "1"),
        (Read::Type("Partial<{ a: 1 }>['a']"), "1 | undefined", "1"),
        (
            Read::Type("Req<{ a: 1 | undefined }>['a']"),
            "1 | undefined",
            "1",
        ),
        (
            Read::Type("Opt<{ a: 1 | undefined }>['a']"),
            "1 | undefined",
            "1",
        ),
        (
            Read::Type("Opt<{ a: void }>['a']"),
            "void | undefined",
            "void",
        ),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Distributive and wrapped conditionals, `infer` in arrays, tuples, signatures
/// and templates, recursion and the identity trick.
const CONDITIONALS: &str = r##"
type IsStr<T> = T extends string ? "yes" : "no";
type Wrapped<T> = [T] extends [string] ? "yes" : "no";
type ElemOf<T> = T extends (infer E)[] ? E : never;
type Unpromise<T> = T extends { then(cb: (v: infer V) => void): void } ? V : T;
type First<T extends unknown[]> = T extends [infer H, ...unknown[]] ? H : never;
type Last<T extends unknown[]> = T extends [...unknown[], infer L] ? L : never;
type Fn1<T> = T extends (a: infer A) => infer R ? [A, R] : never;
type Deep<T> = T extends object ? { [K in keyof T]: Deep<T[K]> } : T;
type InferStr<T> = T extends `${infer H}-${infer R}` ? [H, R] : never;
type Len<T extends readonly unknown[]> = T["length"];
type NotAny<T> = 0 extends 1 & T ? "any" : "not";
type Eq<A, B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type Rev<T extends unknown[]> = T extends [infer H, ...infer R] ? [...Rev<R>, H] : [];
type InferExt<T> = T extends [infer X extends string] ? X : "nope";
type Thenish<T> = T extends object & { then(onfulfilled: infer F, ...args: infer _): any } ? F : "none";
"##;

/// A conditional distributes over a naked union argument (and `never` to
/// `never`, `any` to both branches), a wrapped check does not, and `infer`
/// extracts array elements, tuple heads and tails, signature parameters and
/// results, and tuple lengths.
#[test]
fn conditional_types_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("IsStr<'a'>", "\"yes\""),
        ("IsStr<1>", "\"no\""),
        ("IsStr<'a' | 1>", "\"no\" | \"yes\""),
        ("IsStr<never>", "never"),
        ("IsStr<any>", "\"no\" | \"yes\""),
        ("Wrapped<'a' | 1>", "\"no\""),
        ("Wrapped<never>", "\"yes\""),
        ("ElemOf<string[]>", "string"),
        ("ElemOf<(1 | 2)[]>", "1 | 2"),
        ("ElemOf<number>", "never"),
        ("First<[1, 2, 3]>", "1"),
        ("First<[]>", "never"),
        ("Last<[1, 2, 3]>", "3"),
        ("Fn1<(a: string) => number>", "[string, number]"),
        ("Len<[1, 2]>", "2"),
        ("NotAny<any>", "\"any\""),
        ("NotAny<string>", "\"not\""),
        ("InferExt<['s']>", "\"s\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A check that does not relate to an `infer` pattern even with every
/// `infer` read as `any` takes the false branch, whatever the pattern binds
/// and however deep: `Unpromise<string>` is `string`, `Thenish<{ a: 1 }>`
/// (a `then` method over `object & …`) is `"none"`.
#[test]
fn a_check_no_inference_relates_takes_the_false_branch() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("Unpromise<string>", "string"),
        ("Unpromise<{ a: 1 }>", "{ a: 1; }"),
        ("Unpromise<number[]>", "number[]"),
        ("Thenish<string>", "\"none\""),
        ("Thenish<{ a: 1 }>", "\"none\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `T extends { then(cb: (v: infer V) => void): void } ? V : T` over `{
/// then(cb: (v: 7) => void): void }` is `7`.
///
/// What the lane gives:
/// - `Unpromise<{ then(cb: (v: 7) => void): void }>`: the checker answers `7`;
///   the lane measured `<unreduced conditional>`.
#[test]
#[ignore = "infer inside a method's callback parameter resolves"]
fn infer_from_a_method_callback_parameter_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[("Unpromise<{ then(cb: (v: 7) => void): void }>", "7")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Deep<{ a: { b: 1 } }>` (a conditional mapping each member through itself)
/// is `{ a: { b: 1; }; }`, and `Rev<[1, 2, 3]>` (`[...Rev<R>, H]`) is `[3, 2,
/// 1]`.
///
/// What the lane gives:
/// - `Deep<{ a: { b: 1 } }>`: the checker answers `{ a: { b: 1; }; }`; the lane
///   measured `{ a: <opaque RecursiveRef { name: "Deep", args:
///   [SemanticNodeId(421)] }>; }`; measured `{ a: <opaque RecursiveRef { name:
///   "Deep", args: [SemanticNodeId(440)] }>; }`; measured `{ a: <opaque
///   RecursiveRef { name: "Deep", args: [SemanticNodeId(459)] }>; }`; measured
///   `{ a: <opaque RecursiveRef { name: "Deep", args: [SemanticNodeId(477)] }>;
///   }`.
/// - `Rev<[1, 2, 3]>`: the checker answers `[3, 2, 1]`; the lane measured
///   `[...<opaque RecursiveRef { name: "Rev", args: [SemanticNodeId(789)] }>,
///   1]`; measured `[...<opaque RecursiveRef { name: "Rev", args:
///   [SemanticNodeId(807)] }>, 1]`; measured `[...<opaque RecursiveRef { name:
///   "Rev", args: [SemanticNodeId(825)] }>, 1]`; measured `[...<opaque
///   RecursiveRef { name: "Rev", args: [SemanticNodeId(843)] }>, 1]`.
#[test]
#[ignore = "a conditional alias recursing through a mapped type or a variadic tuple resolves"]
fn a_recursive_conditional_alias_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("Deep<{ a: { b: 1 } }>", "{ a: { b: 1; }; }"),
        ("Rev<[1, 2, 3]>", "[3, 2, 1]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `T extends \`${infer H}-${infer R}\` ? [H, R] : never` over `'a-b-c'` is
/// `["a", "b-c"]` and over `'abc'` is `never`.
///
/// What the lane gives:
/// - `InferStr<'a-b-c'>`: the checker answers `["a", "b-c"]`; the lane measured
///   `<unreduced conditional>`.
/// - `InferStr<'abc'>`: the checker answers `never`; the lane measured
///   `<unreduced conditional>`.
#[test]
#[ignore = "infer placeholders in a template literal check type match leftmost"]
fn infer_in_a_template_literal_pattern_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("InferStr<'a-b-c'>", "[\"a\", \"b-c\"]"),
        ("InferStr<'abc'>", "never"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `(<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ?
/// true : false` is `true` for `1, 1` and `false` for `1, number`.
///
/// What the lane gives:
/// - `Eq<1, 1>`: the checker answers `true`; the lane measured `<unreduced
///   conditional>`.
/// - `Eq<1, number>`: the checker answers `false`; the lane measured
///   `<unreduced conditional>`.
#[test]
#[ignore = "a conditional comparing two generic signatures (the identity check) resolves"]
fn the_generic_signature_identity_check_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[("Eq<1, 1>", "true"), ("Eq<1, number>", "false")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `T extends [infer X extends string] ? X : "nope"` over `[1]` is `"nope"`:
/// the inferred candidate `1` does not satisfy the `infer` constraint.
/// Wrong-but-clean: the lane answers `1`.
///
/// What the lane gives:
/// - `InferExt<[1]>`: the checker answers `"nope"`; the lane measured `1`.
#[test]
#[ignore = "infer X extends C takes the false branch when the candidate is not a C"]
fn wrong_clean_an_infer_constraint_filters_the_candidate() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[("InferExt<[1]>", "\"nope\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A function with optional and rest parameters, an overloaded function, a
/// class and a promise.
const UTILITIES: &str = r##"
declare function f3(a: string, b?: number, ...rest: boolean[]): "r";
declare function ov(a: string): 1;
declare function ov(a: number): 2;
class C3 { constructor(a: string, b: number) {} x = 1 }
declare const pr: Promise<string>;
interface D0 { d: 0 }
"##;

/// `ReturnType` and `Parameters` read a signature (the last overload's),
/// `ConstructorParameters` and `InstanceType` a class, `Exclude` / `Extract` /
/// `NonNullable` filter unions, `Awaited` unwraps nested promises,
/// `ThisParameterType` / `OmitThisParameter` read the `this` parameter, the
/// intrinsic string mappings map literals and templates, and template literal
/// types distribute over their unions.
#[test]
fn utility_types_and_string_mappings_resolve_as_the_checker_resolves_them() {
    let matrix = Matrix::new(UTILITIES);
    let mut failures = matrix.types(&[
        ("ReturnType<typeof f3>", "\"r\""),
        ("ReturnType<typeof ov>", "2"),
        ("Parameters<typeof ov>", "[a: number]"),
        ("ConstructorParameters<typeof C3>", "[a: string, b: number]"),
        ("InstanceType<typeof C3>", "C3"),
        ("Exclude<'a' | 'b' | 1, string>", "1"),
        ("Extract<'a' | 'b' | 1, string>", "\"a\" | \"b\""),
        (
            "Extract<{ k: 1 } | { k: 2 } | string, { k: unknown }>",
            "{ k: 1; } | { k: 2; }",
        ),
        ("NonNullable<string | null | undefined>", "string"),
        ("Awaited<Promise<number>>", "number"),
        ("Awaited<Promise<Promise<'x'>>>", "\"x\""),
        ("Awaited<number>", "number"),
        ("Awaited<typeof pr>", "string"),
        ("ReturnType<() => void>", "void"),
        ("ReturnType<any>", "any"),
        ("ReturnType<never>", "never"),
        ("Parameters<(...a: [1, 2]) => void>", "[1, 2]"),
        ("ThisParameterType<(this: D0, a: 1) => void>", "D0"),
        (
            "OmitThisParameter<(this: D0, a: 1) => void>",
            "(a: 1) => void",
        ),
        ("Uppercase<'ab'>", "\"AB\""),
        ("Lowercase<'AB'>", "\"ab\""),
        ("Capitalize<'ab'>", "\"Ab\""),
        ("Uncapitalize<'AB'>", "\"aB\""),
        ("Uppercase<'a' | 'b'>", "\"A\" | \"B\""),
        ("Capitalize<`x${string}`>", "`X${string}`"),
        ("`${Uppercase<'a'>}-${number}`", "`A-${number}`"),
        (
            "`${'a' | 'b'}${'c' | 'd'}`",
            "\"ac\" | \"ad\" | \"bc\" | \"bd\"",
        ),
        ("`${1 | 2}px`", "\"1px\" | \"2px\""),
        ("`${true}`", "\"true\""),
        ("`${null}`", "\"null\""),
        ("`${string}`", "string"),
    ]);
    failures.extend(matrix.nullness(&[(
        Read::Type("Parameters<typeof f3>"),
        "[a: string, b?: number | undefined, ...rest: boolean[]]",
        "[a: string, b?: number, ...rest: boolean[]]",
    )]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
