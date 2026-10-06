//! The type an inference variable fixes to from candidates of both
//! variances (`getInferredType`).
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting):
//! a type is read off TS2322 for `const s: never = x` with `declare const
//! x: <probe>`, a call off `const s: never = <call>`.

use super::differential_harness_tests::{Matrix, ALL};

const SOURCE: &str = r##"
type MV<T> = T extends { a: infer U; b: (x: infer U) => void } ? U : 0;
type CO<T> = T extends { a: (x: infer U) => void; b: (x: infer U) => void } ? U : 0;
declare function mix<T>(x: T, f: (x: T) => void): T;
declare function mix2<T>(f: (x: T) => void, x: T): T;
declare function co<T>(f: (x: T) => void, g: (x: T) => void): T;
declare function withCallback<T>(cb: (item: T) => T, item: T): T;
declare const one: 1;
declare const s: string;
declare const o: { a: 1 };
declare const sa: "a" | "b";
declare const lit: "literal";
export function c1() { return mix(one, (x: number) => {}); }
export function c2() { return mix(s, (x: "a") => {}); }
export function c3() { return mix2((x: string) => {}, sa); }
export function c4() { return co((x: string) => {}, (x: "a") => {}); }
export function c5() { return co((x: { a: 1 }) => {}, (x: { b: 2 }) => {}); }
export function c6() { return mix(o, (x: { a: number }) => {}); }
export function c7() { return mix(one, (x: string) => {}); }
export function c8() { return withCallback((item: any) => item, lit); }
export function c9() { return withCallback((item) => item, lit); }
"##;

/// A conditional's `infer` fixes to the union of its covariant candidates
/// when it has any, else the intersection of its contravariant ones
/// (`getTypeFromInference`), and the check is then related to the pattern
/// instantiated with it: `{ a: 1; b: (x: string) => void }` infers `1` and
/// fails `{ a: 1; b: (x: 1) => void }`.
#[test]
fn a_conditional_infers_its_covariant_candidates_first() {
    let failures = Matrix::new(SOURCE).settings(&ALL).types(&[
        (r#"MV<{ a: "x"; b: (x: string) => void }>"#, r#""x""#),
        ("MV<{ a: 1; b: (x: string) => void }>", "0"),
        (r#"MV<{ a: "x" | "y"; b: (x: "x") => void }>"#, "0"),
        (
            r#"CO<{ a: (x: string) => void; b: (x: "a") => void }>"#,
            r#""a""#,
        ),
        ("MV<{ a: never; b: (x: string) => void }>", "never"),
        ("MV<{ a: any; b: (x: string) => void }>", "any"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A signature's type parameter prefers its covariant inference when it is
/// neither `never` nor `any` and is assignable to a contravariant
/// candidate, and otherwise takes the contravariant inference — the common
/// subtype of the contravariant candidates, not their intersection
/// (`co((x: { a: 1 }) => {}, (x: { b: 2 }) => {})` is `{ a: 1 }`, with
/// TS2345 on the second argument; `mix(s, (x: "a") => {})` and `mix(one,
/// (x: string) => {})` report TS2345 too).
#[test]
fn a_signature_prefers_its_covariant_inference_where_it_fits() {
    let failures = Matrix::new(SOURCE).settings(&ALL).returns(&[
        ("c1", "1"),
        ("c2", r#""a""#),
        ("c3", r#""a" | "b""#),
        ("c4", r#""a""#),
        ("c5", "{ a: 1; }"),
        ("c6", "{ a: 1; }"),
        ("c7", "string"),
        ("c8", "any"),
        ("c9", r#""literal""#),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A context-sensitive argument is typed under its parameter with the
/// inference fixed, and a fixed inference widens the fresh literal it holds
/// (`getCovariantInference` under `isFixed`): `both(1, (x) => x + 1)` types
/// `x` as `number` and relates without TS2345. Without `strictNullChecks`
/// `T | undefined` is `T`, so `un(undefined)` infers `undefined` for `T`,
/// which widens to `any`.
#[test]
fn a_fixed_inference_widens_and_a_nullish_argument_infers() {
    let source = r##"
declare function both<T>(x: T, f: (x: T) => T): T;
declare function un<T>(x: T | undefined): T;
declare function nu<T>(x: T | null): T;
export function r16() { return both(1, (x) => x + 1); }
export function r12() { return un(undefined); }
export function r12b() { return nu(null); }
"##;
    let reads = [
        (
            super::differential_harness_tests::Read::Return("r16"),
            vec!["number"; 4],
        ),
        (
            super::differential_harness_tests::Read::Return("r12"),
            vec!["undefined", "any", "undefined", "any"],
        ),
        (
            super::differential_harness_tests::Read::Return("r12b"),
            vec!["null", "any", "null", "any"],
        ),
    ];
    let verdicts = Matrix::new(source).settings(&ALL).verdicts(&reads);
    let mut failures = Vec::new();
    for ((read, answers), row) in reads.iter().zip(&verdicts) {
        for (answer, verdict) in answers.iter().zip(row) {
            if !verdict.matched || !verdict.diagnostics.is_empty() {
                failures.push(format!(
                    "`{}`: tsc answers `{answer}` with no diagnostic, {} with {:?}",
                    read.text(),
                    verdict.lane,
                    verdict.diagnostics
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
