//! Relation operands the checker reads through: an intersection target,
//! a homomorphic mapped type, and a `keyof` whose keys settle.
//!
//! TypeScript's `unionOrIntersectionRelatedTo` relates an intersection
//! TARGET arm by arm before it splits an intersection source; a mapped type
//! over a closed object is the object its keys map to (`Partial<Face>` IS
//! `{ a?: 1 }`); and `keyof` over a type whose keys settle is that key set,
//! printed as the keys (`keyof Partial<Face>` is `"a"`) and compared as
//! them. `Extract` / `Exclude` read a mapped constituent the same way.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit --strict` (and, where
//! stated, `--strictNullChecks false`): `declare const v: <probe>; export
//! const s: null = v;` read off the TS2322 message; a `never` answer,
//! which is assignable to `null` and so draws no message, is read as
//! `[<probe>] extends [never] ? 'y' : 'n'` being `"y"`.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, ProbeProject};

const FIXTURE: &str = "\
interface QA { qa: 1 }
interface QB { qb: 2 }
interface Face { a: 1 }
interface Face2 { a: 1; b: 2 }
interface One { a: 1 }
type Obj = { o: 1 };
type Obj2 = { o: 1; p: 2 };
type G<T> = { g: T; h: T };
class D0 { p = 1 as const; private q = 2; protected r = 3; static s = 4 }
interface H { both?: QA; req: QA }
type Div = H & { both?: QB; req?: QB };
interface Rec { next: Rec; v: 1 }
interface Rec2 { next: Rec2; v: 1 }
type PF = Partial<Face | string>;
";

/// An intersection target relates arm by arm, before an intersection
/// source splits, and a member whose type is one relates the same way.
///
/// Measured on TypeScript 7.0.2: `{ qa: 1; qb: 2 } extends QA & QB` is
/// `"y"`, `QA extends QA & QB` and `1 extends QA & QB` are `"n"`, `{ x: QA }
/// extends { x: QA & QB }` is `"n"` and the reverse `"y"`, `((QA | QB) &
/// Obj) extends QA & Obj` is `"n"`, `QA & Obj extends ((QA | QB) & Obj)` is
/// `"y"`, `Div['req'] extends Div['both']` is `"y"` with strict null checks
/// on and off, and `Rec extends Rec2` is `"y"`.
#[test]
fn an_intersection_target_relates_arm_by_arm() {
    let rows = [
        ("{ qa: 1; qb: 2 } extends QA & QB ? 'y' : 'n'", "\"y\""),
        ("QA extends QA & QB ? 'y' : 'n'", "\"n\""),
        ("1 extends QA & QB ? 'y' : 'n'", "\"n\""),
        ("{ x: QA } extends { x: QA & QB } ? 'y' : 'n'", "\"n\""),
        ("{ x: QA & QB } extends { x: QA } ? 'y' : 'n'", "\"y\""),
        ("((QA | QB) & Obj) extends QA & Obj ? 'y' : 'n'", "\"n\""),
        ("QA & Obj extends ((QA | QB) & Obj) ? 'y' : 'n'", "\"y\""),
        ("Div['req'] extends Div['both'] ? 'y' : 'n'", "\"y\""),
        ("Rec extends Rec2 ? 'y' : 'n'", "\"y\""),
    ];
    let failures = mismatches(FIXTURE, &rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let failures = mismatches_in(
        loose,
        FIXTURE,
        &[("Div['req'] extends Div['both'] ? 'y' : 'n'", "\"y\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A homomorphic mapped operand relates as the object it maps to, in the
/// check and the extends position alike.
///
/// Measured on TypeScript 7.0.2: `Partial<Face> extends { a?: 1 }` and `{
/// a?: 1 } extends Partial<Face>` are `"y"`, `Partial<Face> extends { a: 1
/// }` and `Partial<Face> extends { a?: 2 }` are `"n"`, `Required<{ a?: 1 }>
/// extends { a: 1 }` and the reverse are `"y"`, `Readonly<Face> extends
/// Face`, `Readonly<Face> extends { readonly a: 1 }` and `Readonly<{ a: 1 }>
/// extends { a: 1 }` are `"y"`, `Required<Partial<Face>> extends Face` is
/// `"y"` and `Pick<Face, 'a'> extends Face` is `"y"`.
#[test]
fn a_homomorphic_mapped_operand_relates_as_the_object_it_maps_to() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Partial<Face> extends { a?: 1 } ? 'y' : 'n'", "\"y\""),
            ("{ a?: 1 } extends Partial<Face> ? 'y' : 'n'", "\"y\""),
            ("Partial<Face> extends { a: 1 } ? 'y' : 'n'", "\"n\""),
            ("Partial<Face> extends { a?: 2 } ? 'y' : 'n'", "\"n\""),
            ("Required<{ a?: 1 }> extends { a: 1 } ? 'y' : 'n'", "\"y\""),
            ("{ a: 1 } extends Required<{ a?: 1 }> ? 'y' : 'n'", "\"y\""),
            ("Readonly<Face> extends Face ? 'y' : 'n'", "\"y\""),
            (
                "Readonly<Face> extends { readonly a: 1 } ? 'y' : 'n'",
                "\"y\"",
            ),
            ("Readonly<{ a: 1 }> extends { a: 1 } ? 'y' : 'n'", "\"y\""),
            ("Required<Partial<Face>> extends Face ? 'y' : 'n'", "\"y\""),
            ("Pick<Face, 'a'> extends Face ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` over a type whose keys settle is that key set — over a mapped
/// type, a class instance (public members only), a builtin keyed utility
/// and a union — and relates as the keys.
///
/// Measured on TypeScript 7.0.2: `keyof Partial<Face>` is `"a"`, `keyof
/// Readonly<Face | Obj>` is `never`, `keyof D0` is `"p"`,
/// `keyof Required<{ a?: 1; b: 2 }>`, `keyof Pick<Face2, 'a' | 'b'>` and
/// `keyof Record<'x' | 'y', 1>` are the two keys, `keyof (Face2 | One)` is
/// `"a"`; `keyof Face2`, `keyof Partial<Face2>` and `keyof Obj2` extend and
/// are extended by their two keys (`"y"` each way), and `keyof G<1> extends
/// 'g'` and `keyof Required<Face2> extends 'a'` are `"n"`.
#[test]
fn keyof_is_the_key_set_it_settles_to() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("keyof Partial<Face>", "\"a\""),
            ("keyof Readonly<Face | Obj>", "never"),
            ("keyof D0", "\"p\""),
            ("keyof Required<{ a?: 1; b: 2 }>", "\"a\" | \"b\""),
            ("keyof Pick<Face2, 'a' | 'b'>", "\"a\" | \"b\""),
            ("keyof Record<'x' | 'y', 1>", "\"x\" | \"y\""),
            ("keyof (Face2 | One)", "\"a\""),
            ("keyof Face2 extends 'a' | 'b' ? 'y' : 'n'", "\"y\""),
            ("'a' | 'b' extends keyof Face2 ? 'y' : 'n'", "\"y\""),
            (
                "keyof Partial<Face2> extends 'a' | 'b' ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "'a' | 'b' extends keyof Partial<Face2> ? 'y' : 'n'",
                "\"y\"",
            ),
            ("keyof Obj2 extends 'o' | 'p' ? 'y' : 'n'", "\"y\""),
            ("'o' | 'p' extends keyof Obj2 ? 'y' : 'n'", "\"y\""),
            ("keyof G<1> extends 'g' ? 'y' : 'n'", "\"n\""),
            ("keyof Required<Face2> extends 'a' ? 'y' : 'n'", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Extract` and `Exclude` read each constituent as the type it is — a
/// mapped constituent as the object it maps to, a primitive a homomorphic
/// mapping passes through as itself.
///
/// Measured on TypeScript 7.0.2: `Extract<Partial<Face | string>, string>`,
/// `Extract<Partial<Face> | string, string>`, `Extract<{ a?: 1 } | string,
/// string>` and `Extract<PF, string>` are `string`; `Extract<{ a: 1 },
/// object>` is `{ a: 1; }`, `Exclude<{ a: 1 }, object>` is `never`,
/// `Extract<Face, { a: 1 }>` is `Face`, `Extract<[1], unknown[]>
/// extends [1]` is `"y"`, `Exclude<Partial<Face | string>, string>` relates
/// both ways with `{ a?: 1 }` (`"y"`), and `Partial<Face | string> extends
/// string` is `"n"`.
#[test]
fn extract_and_exclude_read_each_constituent_as_the_type_it_is() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Extract<Partial<Face | string>, string>", "string"),
            ("Extract<Partial<Face> | string, string>", "string"),
            ("Extract<{ a?: 1 } | string, string>", "string"),
            ("Extract<PF, string>", "string"),
            ("Extract<{ a: 1 }, object>", "{ a: 1; }"),
            ("Exclude<{ a: 1 }, object>", "never"),
            ("Extract<Face, { a: 1 }>", "Face"),
            ("Extract<[1], unknown[]> extends [1] ? 'y' : 'n'", "\"y\""),
            (
                "Exclude<Partial<Face | string>, string> extends { a?: 1 } ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "{ a?: 1 } extends Exclude<Partial<Face | string>, string> ? 'y' : 'n'",
                "\"y\"",
            ),
            ("Partial<Face | string> extends string ? 'y' : 'n'", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
