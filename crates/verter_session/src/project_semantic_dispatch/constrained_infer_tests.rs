//! An `infer X extends C` declaration: its inference takes the constraint
//! when the constraint refuses it (`getInferredType`), the check is then
//! related to the pattern instantiated with it, and a template literal hole
//! converts its capture by the constraint (`inferToTemplateLiteralType`).
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting):
//! the type is read off TS2322 for `const s: never = x` with `declare const
//! x: <probe>`.

use super::differential_harness_tests::{Matrix, ALL};

/// A bare `infer X extends string` binds a check its constraint accepts and
/// otherwise fails (`1` takes the false branch); an `infer` no source
/// position reaches takes its constraint (`{}` against `{ a?: infer X
/// extends string }` is `string`). A template capture converts to the
/// member of the constraint the checker prefers: `"true"` to `true`,
/// `"12"` to `12n`, `"null"` to `null`, a string a string mapping keeps
/// (`"AB"`) to itself, a string literal before a number (`"1"` under `"1" |
/// 1`), a number before its literal (`1` under `number | 1`); a string that
/// does not round-trip (`"007"`, `"-0"`) converts to nothing, takes the
/// constraint and still relates to the pattern.
#[test]
fn a_constrained_infer_infers_within_its_constraint() {
    let failures = Matrix::new("").settings(&ALL).types(&[
        (r#""a" extends infer X extends string ? X : 0"#, r#""a""#),
        ("1 extends infer X extends string ? X : 0", "0"),
        (
            r#""true" extends `${infer B extends boolean}` ? B : 0"#,
            "true",
        ),
        (r#""12" extends `${infer B extends bigint}` ? B : 0"#, "12n"),
        (
            r#""null" extends `${infer N extends null}` ? N : 0"#,
            "null",
        ),
        (
            r#""AB" extends `${infer U extends Uppercase<string>}` ? U : 0"#,
            r#""AB""#,
        ),
        (
            r#""ab" extends `${infer U extends Uppercase<string>}` ? U : 0"#,
            "0",
        ),
        (
            r#""1" extends `${infer V extends "1" | 1}` ? V : 0"#,
            r#""1""#,
        ),
        (
            r#""1" extends `${infer V extends number | 1}` ? V : 0"#,
            "1",
        ),
        (
            "{} extends { a?: infer X extends string } ? X : 0",
            "string",
        ),
        (
            r#""007" extends `${infer N extends number}` ? N : 0"#,
            "number",
        ),
        (
            r#""-0" extends `${infer B extends bigint}` ? B : 0"#,
            "bigint",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A conditional the checker keeps over a generic check keeps its
/// constrained `infer` declarations, printed with their constraints.
#[test]
fn a_deferred_conditional_keeps_its_constrained_infer() {
    let source = r##"
export function d1<T>() { return null! as (T extends [infer X extends string] ? X : 0); }
export function d2<T>() { return null! as (T extends `${infer N extends number}px` ? N : 0); }
"##;
    let failures = Matrix::new(source).settings(&ALL).returns(&[
        ("d1", "T extends [infer X extends string] ? X : 0"),
        ("d2", "T extends `${infer N extends number}px` ? N : 0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An `infer` that declares no constraint takes the one its position
/// implies (`getInferredTypeParameterConstraint`): a reference's type
/// argument the constraint of the reference's type parameter (`{ v: 1 }`
/// against `B<infer U>` over `type B<X extends string>` fixes `string` and
/// fails; an uninferred one is `string`), a rest element or rest parameter
/// `unknown[]` (a rest capture is a mutable slice, which it accepts), a
/// template literal hole `string`.
#[test]
fn an_infer_takes_the_constraint_its_position_implies() {
    let source = r##"
type B<X extends string> = { v: X };
interface IB<X extends string> { v: X }
"##;
    let failures = Matrix::new(source).settings(&ALL).types(&[
        ("{ v: 1 } extends B<infer U> ? U : 0", "0"),
        (r#"B<"a"> extends B<infer U> ? U : 0"#, r#""a""#),
        (r#"{ v: "a" } extends B<infer U> ? U : 0"#, r#""a""#),
        ("{} extends { v?: B<infer U> } ? U : 0", "string"),
        (r#"IB<"a"> extends IB<infer U> ? U : 0"#, r#""a""#),
        ("[] extends [B<infer U>?] ? U : 0", "string"),
        (
            "{} extends { f?: (...a: infer R) => void } ? R : 0",
            "unknown[]",
        ),
        ("{} extends { t?: `${infer S}` } ? S : 0", "string"),
        ("{} extends { r?: [1, ...infer T] } ? T : 0", "unknown[]"),
        // A rest capture is a mutable slice, so a readonly source passes
        // the implied `unknown[]`.
        (
            "readonly [1, 2] extends readonly [1, ...infer R] ? R : never",
            "[2]",
        ),
        (
            "((...a: readonly [1]) => void) extends (...a: infer R) => void ? R : never",
            "[1]",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
