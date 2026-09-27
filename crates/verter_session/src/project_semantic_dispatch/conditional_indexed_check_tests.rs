//! A conditional whose check type is an indexed access. The checker
//! instantiates the check type where the conditional is written, so an
//! indexed access over a type that is not generic IS the property type it
//! reads: `any` there takes both branches (whether or not the conditional
//! distributes, since `T[K]` is never a naked type parameter), and a tuple
//! or array wrapping the read relates the property type too.
//!
//! Every expected answer is TypeScript 7.0.2's, read off the TS2322 message
//! of `declare const p: <probe>; export const s: null = p;` (and
//! `IsAny<…>` for an `any` answer) under each `strictNullChecks` ×
//! `noImplicitAny` setting; the four settings agree.

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
type Rec = { a: any; b: string; n: never; u: unknown };
type D<T, K extends keyof T> = T[K] extends string ? 1 : 2;
type ND<T, K extends keyof T> = [T[K]] extends [string] ? 1 : 2;
type DK<T> = T[keyof T] extends string ? 1 : 2;
type DX<T> = T[\"x\" & keyof T] extends string ? 1 : 2;
type DKey<T, K extends keyof T> = T[K] extends string ? T[K] : K;
type E<T> = T extends string ? 1 : 2;
type EW<T> = [T] extends [string] ? 1 : 2;
";

/// An `any` read through an indexed access takes both branches.
///
/// Measured on TypeScript 7.0.2:
///
/// | probe | checker |
/// | --- | --- |
/// | `D<Rec, "a">`, `Rec["a"] extends string ? 1 : 2`, `DK<{ a: any }>`, `DK<{ a: any; b: number }>`, `D<Rec, "a" \| "b">`, `DX<{ x: any }>`, `E<Rec["a"]>` | `1 \| 2` |
/// | `DKey<Rec, "a">` | `any` |
/// | `D<Rec, "b">`, `D<Rec, "n">` | `1` |
/// | `D<Rec, "u">` | `2` |
#[test]
fn an_indexed_access_check_reading_any_takes_both_branches() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("D<Rec, \"a\">", "1 | 2"),
            ("Rec[\"a\"] extends string ? 1 : 2", "1 | 2"),
            ("DK<{ a: any }>", "1 | 2"),
            ("DK<{ a: any; b: number }>", "1 | 2"),
            ("D<Rec, \"a\" | \"b\">", "1 | 2"),
            ("DX<{ x: any }>", "1 | 2"),
            ("E<Rec[\"a\"]>", "1 | 2"),
            ("DKey<Rec, \"a\">", "any"),
            ("D<Rec, \"b\">", "1"),
            ("D<Rec, \"n\">", "1"),
            ("D<Rec, \"u\">", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A non-distributive check wrapping the read relates the property type:
/// `[any]` and `[never]` are both assignable to `[string]`.
///
/// Measured on TypeScript 7.0.2: `ND<Rec, "a">`, `ND<Rec, "n">`,
/// `ND<Rec, "a" | "b">`, `EW<Rec["a"]>`, `[Rec["a"]] extends [string] ? 1 :
/// 2`, `[Rec["b"]] extends [string] ? 1 : 2` and `Rec["b"][] extends
/// string[] ? 1 : 2` are `1`.
#[test]
fn a_wrapped_indexed_access_check_relates_the_property_type() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ND<Rec, \"a\">", "1"),
            ("ND<Rec, \"n\">", "1"),
            ("ND<Rec, \"a\" | \"b\">", "1"),
            ("EW<Rec[\"a\"]>", "1"),
            ("[Rec[\"a\"]] extends [string] ? 1 : 2", "1"),
            ("[Rec[\"b\"]] extends [string] ? 1 : 2", "1"),
            ("Rec[\"b\"][] extends string[] ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
