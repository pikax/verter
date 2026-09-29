//! A conditional's check and extends types are the types the checker
//! constructs before it relates them: a union or intersection whose members
//! name a lattice extreme is that extreme (`1 & T` over `T = any` is `any`),
//! whatever carrier the composite was built as, and an operand that is the
//! checker's error type is the conditional's answer.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on [`FIXTURE`], each probe read off a
//! TS2322 against `never`. The four `strictNullChecks` × `noImplicitAny`
//! settings agree on every probe. The checker prints its error type (here
//! the TS2589 recovery of `Same<0>` and `Rep<1000>`) as `any`.

use super::checker_probe_lane_tests::mismatches_in_one_host;

const FIXTURE: &str = r#"
type IsAny<T> = 0 extends 1 & T ? "any" : "not-any";
type A = any;
type U = unknown;
type Rep<N extends number, Acc extends unknown[] = []> = Acc["length"] extends N ? Acc : Rep<N, [...Acc, 0]>;
type Same<T> = T extends 0 ? Same<T> : 1;
type E = Same<0>;
type Box<T> = { v: T };
type InU<T> = 0 extends 1 | T ? "in" : "out";
type Nev<T> = 0 extends 1 & T & never ? "y" : "n";
"#;

/// An intersection substituted into an instantiated body is the intersection
/// the checker constructs: `1 & T` over `any` is `any`, over the error type
/// the error type, and `never` beside `any` wins.
///
/// Measured: `IsAny<A>`, `IsAny<any>` and `IsAny<Box<any>["v"]>` are
/// `"any"`; `IsAny<U>`, `IsAny<never>` and `IsAny<string>` are
/// `"not-any"`; `IsAny<E>` and `IsAny<Rep<1000>>` are `any` (the error
/// type); `InU<A>` is `"in"`, `InU<E>` is `any`; `Nev<A>` is `"n"`.
#[test]
fn a_composite_operand_is_the_extreme_its_members_name() {
    let failures = mismatches_in_one_host(
        FIXTURE,
        &[
            ("IsAny<A>", r#""any""#),
            ("IsAny<any>", r#""any""#),
            ("IsAny<Box<any>[\"v\"]>", r#""any""#),
            ("IsAny<U>", r#""not-any""#),
            ("IsAny<never>", r#""not-any""#),
            ("IsAny<string>", r#""not-any""#),
            ("IsAny<E>", "any"),
            ("IsAny<Rep<1000>>", "any"),
            ("InU<A>", r#""in""#),
            ("InU<E>", "any"),
            ("Nev<A>", r#""n""#),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
