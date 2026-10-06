//! Inference from a generic source signature reads its base signature.
//!
//! The checker's `inferFromSignatures` infers from `getBaseSignature` of
//! each source signature: its type parameters at their constraints
//! (`unknown` without one), so a type parameter of the source never
//! reaches an inferred type. Every expected answer is TypeScript 7.0.2's,
//! measured with `tsc --ignoreConfig --noEmit --strict --noErrorTruncation`
//! under all four `strictNullChecks` x `noImplicitAny` settings (alike in
//! every setting): the type is read off TS2322 for `const s: never = x`
//! with `declare const x: <probe>`.

use super::differential_harness_tests::{Matrix, ALL};

const SOURCE: &str = r##"
type G = <T>(x: T) => T;
type G2 = <T extends string>(x: T) => T[];
type G5 = <A, B extends A>(a: A, b: B) => B;
type GM = { m<T>(x: T): T };
type RT<F> = F extends (...a: any) => infer R ? R : any;
"##;

/// `<T>(x: T) => T` against `(...a: infer A) => infer R` infers `[x:
/// unknown]` and `unknown`, a constrained parameter its constraint, and a
/// parameter bounded by another the other's base; a generic method infers
/// the same way. The relation that follows inference still decides the
/// branch: `(x: unknown) => unknown` does not return `number`.
#[test]
fn a_generic_source_signature_infers_from_its_base_signature() {
    let failures = Matrix::new(SOURCE).settings(&ALL).types(&[
        (
            "G extends (...a: infer A) => infer R ? [A, R] : 0",
            "[[x: unknown], unknown]",
        ),
        (
            "G2 extends (...a: infer A) => infer R ? [A, R] : 0",
            "[[x: string], string[]]",
        ),
        ("G extends (x: infer X) => any ? X : 0", "unknown"),
        ("RT<G2>", "string[]"),
        (
            "G5 extends (...a: infer A) => infer R ? [A, R] : 0",
            "[[a: unknown, b: unknown], unknown]",
        ),
        (
            "GM extends { m(x: infer X): infer Y } ? [X, Y] : 0",
            "[unknown, unknown]",
        ),
        ("G extends (x: string) => infer R ? R : 0", "unknown"),
        ("G extends (x: infer X) => number ? X : 0", "0"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
