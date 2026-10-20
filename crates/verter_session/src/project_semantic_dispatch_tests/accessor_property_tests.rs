//! Accessors of an interface or class. A `get` / `set` pair is ONE
//! property to every reader: a read gives the getter's type, a mapped type
//! maps it as a property — `readonly` when there is no setter, mutable when
//! there is one, never the accessor function — and an `accessor` field is a
//! mutable property its class's `keyof` and indexed reads name. Methods and
//! call signatures stay what they are.
//!
//! Every expected answer is TypeScript 7.0.2's, read through its native
//! API (`getTypeAtPosition` + `typeToString` on the alias) in a `strict`
//! project.

use super::checker_probe_lane_tests::{
    evaluated_member_modifiers, evaluated_mismatches, mismatches,
};

const FIXTURE: &str = "\
interface R<T, S = T> { get value(): T; set value(_: S); }
declare const r: R<number, string>;
interface G { get r(): \"s\" }
type Id<T> = { [K in keyof T]: T[K] };
type Mut<T> = { -readonly [K in keyof T]: T[K] };
type Req<T> = { [K in keyof T]-?: T[K] };
declare class C { get r(): \"s\"; get rw(): 1; set rw(v: 1); m(): 2; accessor f: 3; static accessor s: 4; }
interface GS { get a(): 1; set a(v: 1); set w(v: 2); m(): 3; }
interface CS { (): 5; get v(): 6; }
class D { accessor n = 1; accessor t: \"x\" | \"y\" = \"x\"; static accessor z = \"s\"; }
class Box<T> { constructor(private v: T) {} get value(): T { return this.v; } set value(x: T) { this.v = x; } }
declare const b: Box<string>;
declare const c: C;
interface N { get 1(): \"s\"; set \"1\"(v: \"s\"); }
declare function id<T>(v: T): T;
class Pt { x = 1; }
class E { a = 1; accessor generic = id(1); accessor fieldThis = this.a; accessor p = new Pt(); accessor f = () => 1; static accessor sp = new Pt(); plain = id(1); }
";

/// A mapped type over a type with accessors maps each accessor as the
/// property it is: a getter alone is `readonly`, a get / set pair and a
/// setter alone are mutable, each at its read type. The named demand is
/// the semantic benchmark's `core-accessor-mapping` scenario.
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `{ [K in keyof G]: G[K] }`, `Id<G>`, `Pick<G, "r">` | `{ readonly r: "s"; }` |
/// | `Mut<G>` | `{ r: "s"; }` |
/// | `Id<C>` | `{ readonly r: "s"; rw: 1; m: () => 2; f: 3; }` |
/// | `Pick<C, "rw" \| "f">` | `{ rw: 1; f: 3; }` |
/// | `Omit<GS, "m">` | `{ a: 1; w: 2; }` |
/// | `Id<GS>`, `Req<GS>` | `{ a: 1; w: 2; m: () => 3; }` |
/// | `Readonly<GS>` | `{ readonly a: 1; readonly w: 2; readonly m: () => 3; }` |
/// | `Id<CS>` | `{ readonly v: 6; }` |
/// | `Id<D>` | `{ n: number; t: "x" \| "y"; }` |
#[test]
fn a_mapping_maps_an_accessor_as_its_property() {
    let answer = super::mapped_expand_tests::named_answer(
        "interface G { get r(): \"s\" }\n",
        "{ [K in keyof G]: G[K] }",
    );
    assert_eq!(
        super::mapped_expand_tests::display(&answer.answer),
        "{ readonly r: 's' }",
        "the named mapping over a getter is the checker's readonly property"
    );
    let mut failures = mismatches(
        FIXTURE,
        &[("{ [K in keyof G]: G[K] }", "{ readonly r: \"s\"; }")],
    );
    failures.extend(evaluated_mismatches(
        FIXTURE,
        &[
            ("Id<G>", "{ readonly r: \"s\"; }"),
            ("Pick<G, \"r\">", "{ readonly r: \"s\"; }"),
            ("Pick<C, \"rw\" | \"f\">", "{ rw: 1; f: 3; }"),
            ("Omit<GS, \"m\">", "{ a: 1; w: 2; }"),
            ("Mut<G>", "{ r: \"s\"; }"),
            ("Id<C>", "{ readonly r: \"s\"; rw: 1; m: () => 2; f: 3; }"),
            ("Id<GS>", "{ a: 1; w: 2; m: () => 3; }"),
            ("Req<GS>", "{ a: 1; w: 2; m: () => 3; }"),
            (
                "Readonly<GS>",
                "{ readonly a: 1; readonly w: 2; readonly m: () => 3; }",
            ),
            ("Id<CS>", "{ readonly v: 6; }"),
            ("Id<D>", "{ n: number; t: \"x\" | \"y\"; }"),
        ],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A getter and a setter whose keys spell one property (`1` and `"1"`)
/// are one accessor: a mapping over them publishes ONE property, mutable
/// because it has a setter.
///
/// Measured on TypeScript 7.0.2: `Id<N>` is `{ 1: "s"; }`, and
/// `declare const n: Id<N>; n[1] = "s";` compiles.
#[test]
fn equivalent_accessor_key_spellings_map_as_one_mutable_property() {
    let members = evaluated_member_modifiers(FIXTURE, "Id<N>", "\"s\"");
    assert_eq!(
        members,
        vec![(false, true)],
        "`get 1` / `set \"1\"` map as one mutable `\"s\"` property, (readonly, value matches) per member"
    );
}

/// A read of an accessor reads its property type, through the
/// instantiation of a generic interface or class; a setter alone reads its
/// parameter; a method reads as the method. These are the reads the
/// semantic benchmark's `vue-accessor-ref-read` scenario makes (Vue 3.5's
/// `Ref<T, S>.value`).
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `typeof r.value` | `number` |
/// | `R<boolean, string>['value']` | `boolean` |
/// | `typeof b.value` | `string` |
/// | `Box<number>['value']` | `number` |
/// | `typeof c.rw` | `1` |
/// | `GS['w']` | `2` |
/// | `GS['m']` | `() => 3` |
/// | `ReturnType<GS['m']>` | `3` |
#[test]
fn an_accessor_read_is_its_property_type() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("typeof r.value", "number"),
            ("R<boolean, string>['value']", "boolean"),
            ("typeof b.value", "string"),
            ("Box<number>['value']", "number"),
            ("typeof c.rw", "1"),
            ("GS['w']", "2"),
            ("GS['m']", "() => 3"),
            ("ReturnType<GS['m']>", "3"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `accessor` field is a mutable property of its declared type, else
/// its initializer's widened type: its class's `keyof` names it and an
/// indexed or member read reads it, on the instance and the constructor.
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `C['f']`, `typeof c.f` | `3` |
/// | `(typeof C)['s']` | `4` |
/// | `D['n']` | `number` |
/// | `D['t']` | `"x" \| "y"` |
/// | `(typeof D)['z']` | `string` |
/// | `Exclude<keyof C, "r" \| "rw" \| "m">` | `"f"` |
/// | `Exclude<keyof D, "n">` | `"t"` |
/// | `E['generic']`, `E['fieldThis']`, `E['plain']` | `number` |
/// | `E['p']`, `(typeof E)['sp']` | `Pt` |
/// | `E['f']` | `() => number` |
#[test]
fn an_accessor_field_is_a_property_of_its_class() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("C['f']", "3"),
            ("typeof c.f", "3"),
            ("(typeof C)['s']", "4"),
            ("D['n']", "number"),
            ("D['t']", "\"x\" | \"y\""),
            ("(typeof D)['z']", "string"),
            ("Exclude<keyof C, \"r\" | \"rw\" | \"m\">", "\"f\""),
            ("Exclude<keyof D, \"n\">", "\"t\""),
            ("E['generic']", "number"),
            ("E['fieldThis']", "number"),
            ("E['plain']", "number"),
            ("E['p']", "Pt"),
            ("(typeof E)['sp']", "Pt"),
            ("E['f']", "() => number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
