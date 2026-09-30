//! A conditional's `extends` pattern infers every `infer` declaration its
//! structure reaches — object members, index signatures, call and construct
//! signatures, parameters, returns and predicates, array and tuple
//! elements, union and intersection members, references — and keeps any
//! other position an explicit gap.
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting):
//! the type is read off TS2322 for `const s: never = x` with `declare const
//! x: <probe>`.

use super::differential_harness_tests::{Matrix, Read, ALL};

/// Declarations nested in the structure of a pattern infer as the checker
/// infers them, at any depth.
#[test]
fn a_pattern_infers_every_declaration_its_structure_reaches() {
    let failures = Matrix::new("").settings(&ALL).types(&[
        (
            "{ [k: string]: string } extends { [k: string]: infer U } ? U : never",
            "string",
        ),
        (
            "{ (value: string): string } extends { (value: string): infer U } ? U : never",
            "string",
        ),
        (
            "{ new (value: string): string } extends { new (value: string): infer U } ? U : never",
            "string",
        ),
        (
            "{ [k: string]: 1; a: 1 } extends { [k: string]: infer U } ? U : never",
            "1",
        ),
        (
            r#"{ a: 1; b: "x" } extends { [k: string]: infer U } ? U : never"#,
            r#""x" | 1"#,
        ),
        (
            "{ [k: number]: 1 } extends { [k: string]: infer U } ? U : never",
            "1",
        ),
        ("{ a: 1 }[] extends { a: infer U }[] ? U : 0", "1"),
        ("(() => { a: 1 }) extends () => { a: infer U } ? U : 0", "1"),
        (
            "((x: { a: 1 }) => void) extends (x: { a: infer U }) => void ? U : 0",
            "1",
        ),
        (
            "((x: unknown) => x is { a: 1 }) extends (x: unknown) => x is { a: infer U } ? U : 0",
            "1",
        ),
        (
            "{ a: 1 } & { b: 2 } extends { a: infer U } & { b: infer V } ? [U, V] : 0",
            "[1, 2]",
        ),
        (
            "{ a: 1 } extends { a: infer U } & { a: infer V } ? [U, V] : 0",
            "[1, 1]",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A declaration below structure the relation does not infer through is an
/// explicit gap, never an answer: the checker infers `{ a: any }` for `"a"`
/// against `keyof infer K`, and `string` for `"AB"` against
/// `Uppercase<infer U>` (the constraint the string mapping's parameter
/// implies), inferences the lane does not make.
#[test]
fn a_declaration_below_keyof_or_a_builtin_utility_is_a_gap() {
    let reads: Vec<(Read<'_>, Vec<&str>)> = vec![
        (
            Read::Type(r#""a" extends keyof infer K ? K : 0"#),
            vec!["{ a: any; }"; ALL.len()],
        ),
        (
            Read::Type(r#""AB" extends Uppercase<infer U> ? U : 0"#),
            vec!["string"; ALL.len()],
        ),
    ];
    let verdicts = Matrix::new("").settings(&ALL).verdicts(&reads);
    for (row, (read, _)) in verdicts.iter().zip(&reads) {
        for verdict in row {
            assert!(
                !verdict.matched && verdict.class == "GAP",
                "`{}` is undecided, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}
