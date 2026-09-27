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
/// & K>}\`]: () => T[K]` is `() => 1`: the selected member's value reduces
/// the indexed access its key closes.
#[test]
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
#[test]
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
type OnName<T> = T extends { on(h: (e: infer E) => void): void; name: infer N } ? [E, N] : 0;
type MethodRet<T> = T extends { f(): infer R } ? R : 0;
type MemberElem<T> = T extends { a: (infer U)[] } ? U : 0;
type First<T extends unknown[]> = T extends [infer H, ...unknown[]] ? H : never;
type Last<T extends unknown[]> = T extends [...unknown[], infer L] ? L : never;
type Fn1<T> = T extends (a: infer A) => infer R ? [A, R] : never;
type Deep<T> = T extends object ? { [K in keyof T]: Deep<T[K]> } : T;
type InferStr<T> = T extends `${infer H}-${infer R}` ? [H, R] : never;
type InferTail<T> = T extends `x${infer R}` ? R : 0;
type InferTwo<T> = T extends `${infer A}${infer B}` ? [A, B] : 0;
type InferAfterNum<T> = T extends `${number}-${infer R}` ? R : 0;
type Len<T extends readonly unknown[]> = T["length"];
type NotAny<T> = 0 extends 1 & T ? "any" : "not";
type Eq<A, B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type Rev<T extends unknown[]> = T extends [infer H, ...infer R] ? [...Rev<R>, H] : [];
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
/// then(cb: (v: 7) => void): void }` is `7`: an `infer` placeholder nested
/// in a member through structure alone (a method's callback parameter, a
/// method's return, an array element) is inferred by the structural
/// relation, beside a direct member placeholder.
#[test]
fn infer_from_a_method_callback_parameter_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("Unpromise<{ then(cb: (v: 7) => void): void }>", "7"),
        (
            "OnName<{ on(h: (e: \"x\") => void): void; name: \"n\" }>",
            "[\"x\", \"n\"]",
        ),
        ("MethodRet<{ f(): 5 }>", "5"),
        ("MemberElem<{ a: string[] }>", "string"),
        (
            "Unpromise<{ then(cb: string): void }>",
            "{ then(cb: string): void; }",
        ),
        ("MemberElem<{ a: number }>", "0"),
    ]);
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
/// `["a", "b-c"]` and over `'abc'` is `never`: each placeholder takes the
/// slice of the source its hole covers, texts matched leftmost, a
/// placeholder before an empty text one character, and a settled hole
/// (`${number}`) checks its slice.
#[test]
fn infer_in_a_template_literal_pattern_resolves() {
    let matrix = Matrix::new(CONDITIONALS);
    let failures = matrix.types(&[
        ("InferStr<'a-b-c'>", "[\"a\", \"b-c\"]"),
        ("InferStr<'abc'>", "never"),
        ("InferTail<'xyz'>", "\"yz\""),
        ("InferTail<'abc'>", "0"),
        ("InferTwo<'abc'>", "[\"a\", \"bc\"]"),
        ("InferTwo<''>", "0"),
        ("InferAfterNum<'12-ab'>", "\"ab\""),
        ("InferAfterNum<'x-ab'>", "0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Template literal patterns over sources that are no string literal.
const TEMPLATE_PATTERN_SOURCES: &str = r##"
type T10<S> = S extends `a${infer H}` ? H : "none";
type T12<S> = S extends `${infer H}-${number}` ? H : "none";
"##;

/// A source the pattern cannot slice takes the false branch when it is not
/// below the pattern even with each placeholder read as `string`: `string`
/// and `number` against `a${infer H}`, `boolean` against `${infer H}-${number}`.
/// Measured on TypeScript 7.0.2, alike under all four settings.
#[test]
fn a_source_no_placeholder_reading_fits_takes_the_false_branch() {
    let matrix = Matrix::new(TEMPLATE_PATTERN_SOURCES);
    let failures = matrix.types(&[
        ("T10<string>", "\"none\""),
        ("T10<number>", "\"none\""),
        ("T12<boolean>", "\"none\""),
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

/// Conditionals over constrained `infer` placeholders.
const CONSTRAINED_INFER: &str = r##"
type InferExt<T> = T extends [infer X extends string] ? X : "nope";
type T3<S> = S extends `${infer N extends number}px` ? N : "none";
type T8<S> = S extends `${infer B extends boolean}!` ? B : "none";
type T9<S> = S extends `${infer N extends 1 | 2}` ? N : "none";
"##;

/// The checker's reading of a constrained `infer X extends C`, measured on
/// TypeScript 7.0.2 alike under all four settings: a candidate the
/// constraint refuses takes the false branch (`InferExt<[1]>` is `"nope"`),
/// and a template slice parses as a literal of the constraint when it
/// round-trips (`"12"` is `12`, `"true"` is `true`, `"2"` is `2`); one that
/// does not (`"1e3"`, `" 1"`) is the constraint, and a slice no member of
/// the constraint parses (`"3"` against `1 | 2`) takes the false branch.
const CONSTRAINED_INFER_ROWS: [(&str, &str); 8] = [
    ("InferExt<[1]>", "\"nope\""),
    ("InferExt<[\"a\"]>", "\"a\""),
    ("T3<\"12px\">", "12"),
    ("T3<\"1e3px\">", "number"),
    ("T3<\" 1px\">", "number"),
    ("T8<\"true!\">", "true"),
    ("T9<\"2\">", "2"),
    ("T9<\"3\">", "\"none\""),
];

/// Each constrained-`infer` row that does not read the checker's answer,
/// only those the lane publishes clean when `clean_only`.
fn constrained_infer_misses(clean_only: bool) -> Vec<String> {
    let matrix = Matrix::new(CONSTRAINED_INFER);
    let rows: Vec<(Read<'_>, Vec<&str>)> = CONSTRAINED_INFER_ROWS
        .iter()
        .map(|(text, answer)| (Read::Type(text), vec![*answer; 4]))
        .collect();
    CONSTRAINED_INFER_ROWS
        .iter()
        .zip(matrix.verdicts(&rows))
        .flat_map(|((text, _), verdicts)| {
            verdicts
                .into_iter()
                .filter(|verdict| !verdict.matched && (!clean_only || verdict.class != "GAP"))
                .map(move |verdict| format!("`{text}`: {}", verdict.lane))
        })
        .collect()
}

/// The lane carries no `infer` constraint, so a constrained placeholder, in
/// a tuple or a template literal pattern, degrades rather than reading as
/// unconstrained.
#[test]
fn a_constrained_infer_placeholder_is_never_read_as_unconstrained() {
    let wrong = constrained_infer_misses(true);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What the lane gives: each row degrades (the constrained placeholder is
/// unsupported syntax).
#[test]
#[ignore = "infer X extends C filters and parses its candidate by the constraint"]
fn an_infer_constraint_filters_and_parses_the_candidate() {
    let failures = constrained_infer_misses(false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `infer` placeholders beside a fixed member of a union pattern.
const INFER_IN_A_UNION: &str = r##"
type UA<T> = T extends { a: infer X | string } ? X : "none";
"##;

/// A source member the pattern's fixed `string` matches (by identity or by
/// its base) infers nothing to the placeholder beside it: `number | string`
/// infers `number`, `"x" | 1 | true` infers `1 | true`. A source every
/// member of which matches infers the whole source, below any direct
/// inference: `string` alone is `string` and `"x"` alone `"x"`. Measured
/// on TypeScript 7.0.2, alike under all four settings.
#[test]
fn an_infer_placeholder_beside_a_fixed_union_member_takes_the_unmatched_members() {
    let matrix = Matrix::new(INFER_IN_A_UNION);
    let failures = matrix.types(&[
        ("UA<{ a: number | string }>", "number"),
        ("UA<{ a: string }>", "string"),
        ("UA<{ a: \"x\" }>", "\"x\""),
        ("UA<{ a: number }>", "number"),
        ("UA<{ a: \"x\" | 1 | true }>", "1 | true"),
    ]);
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
