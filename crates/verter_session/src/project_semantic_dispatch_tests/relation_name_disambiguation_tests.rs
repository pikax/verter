//! Two types whose messages would spell the same text are told apart the
//! way the checker tells them apart (`getTypeNamesForErrorDisplay`): both
//! print fully qualified, and only types that still agree take TS2719's
//! message. The expected reports are TypeScript 7.1.0-beta's, measured with
//! `tsc --pretty false --strict` on this exact fixture rooted at `/wb`.

use std::sync::Arc;

use super::signature_predicate_inference_tests::{checker_report, host_with, STRICT_ROOT};
use crate::VerterHost;

const A: &str = r#"export interface Foo { p: number }
export interface Gen<T> { v: T; p: number }
interface Hidden { h: number }
export declare const hidden: Hidden;
export namespace NS { export interface Inner { p: number } }
interface Renamed { p: number }
export { Renamed as Public };
export default interface Dflt { p: number }
export enum E { X, Y }
export enum S { Only }
"#;

const B: &str = r#"export interface Foo { p: string }
export interface Gen<T> { v: T; p: string }
interface Hidden { h: string }
export declare function takeHidden(x: { q: Hidden }): void;
export declare function takeHiddens(x: { q: Hidden[] }): void;
export namespace NS { export interface Inner { p: string } }
interface Renamed { p: string }
export { Renamed as Public };
export default interface Dflt { p: string }
export enum E { X, Z }
export enum S { Solo }
"#;

const MAIN: &str = r#"import { hidden } from './a';
import type { Foo as A, Gen as GA, NS as NA, Public as PA } from './a';
import type DA from './a';
import { takeHidden, takeHiddens } from './b';
import type { Foo as B, Gen as GB, NS as NB, Public as PB } from './b';
import type DB from './b';
declare function f(x: { q: B }): void;
export function g1(a: A) { return f({ q: a }); }
declare function f2(x: B): void;
export function g2(a: A) { return f2(a); }
declare function f3(x: { q: GB<number> }): void;
export function g3(a: GA<number>) { return f3({ q: a }); }
export function g4() { return takeHidden({ q: hidden }); }
declare function f5(x: { q: NB.Inner }): void;
export function g5(a: NA.Inner) { return f5({ q: a }); }
declare function f6(x: { q: PB }): void;
export function g6(a: PA) { return f6({ q: a }); }
declare function f7(x: { q: DB }): void;
export function g7(a: DA) { return f7({ q: a }); }
declare function f8(x: { q: { n: B } }): void;
export function g8(a: { n: A }) { return f8({ q: a }); }
declare function f9(x: { q: B[] }): void;
export function g9(a: A[]) { return f9({ q: a }); }
export function g11() { return takeHiddens({ q: [hidden] }); }
import type { E as EA, S as SA } from './a';
import type { E as EB, S as SB } from './b';
declare function f12(x: { q: EB }): void;
export function g12(a: EA) { return f12({ q: a }); }
declare function f13(x: { q: SB }): void;
export function g13(a: SA) { return f13({ q: a }); }
interface Box<T> { v: T }
declare function f14(x: { q: Box<B> }): void;
export function g14(a: Box<A>) { return f14({ q: a }); }
declare function f16(x: { q: { a: B; b: B; c: B; d: B; e: B; f: B; g: B } }): void;
export function g16(a: { a: A; b: A; c: A; d: A; e: A; f: A; g: A }) { return f16({ q: a }); }
"#;

/// The checker's report for each function of [`MAIN`], rooted at `/wb`.
const EXPECTED: &[(&str, &str)] = &[
    (
        "g1",
        r#"(8,39): error TS2322: Type 'import("/wb/a").Foo' is not assignable to type 'import("/wb/b").Foo'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g2",
        r#"(10,38): error TS2345: Argument of type 'import("/wb/a").Foo' is not assignable to parameter of type 'import("/wb/b").Foo'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g3",
        r#"(12,49): error TS2322: Type 'import("/wb/a").Gen<number>' is not assignable to type 'import("/wb/b").Gen<number>'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g4",
        r#"(13,44): error TS2719: Type 'Hidden' is not assignable to type 'Hidden'. Two different types with this name exist, but they are unrelated.
  Types of property 'h' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g5",
        r#"(15,47): error TS2322: Type 'import("/wb/a").NS.Inner' is not assignable to type 'import("/wb/b").NS.Inner'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g6",
        r#"(17,41): error TS2322: Type 'import("/wb/a").Public' is not assignable to type 'import("/wb/b").Public'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g7",
        r#"(19,41): error TS2322: Type 'import("/wb/a").default' is not assignable to type 'import("/wb/b").default'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g8",
        r#"(21,47): error TS2322: Type '{ n: import("/wb/a").Foo; }' is not assignable to type '{ n: import("/wb/b").Foo; }'.
  The types of 'n.p' are incompatible between these types.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g9",
        r#"(23,42): error TS2322: Type 'import("/wb/a").Foo[]' is not assignable to type 'import("/wb/b").Foo[]'.
  Type 'import("/wb/a").Foo' is not assignable to type 'import("/wb/b").Foo'.
    Types of property 'p' are incompatible.
      Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g11",
        r#"(24,50): error TS2719: Type 'Hidden' is not assignable to type 'Hidden'. Two different types with this name exist, but they are unrelated.
  Types of property 'h' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g16",
        r#"(35,85): error TS2322: Type '{ a: import("/wb/a").Foo; b: import("/wb/a").Foo; c: import("/wb/a").Foo; d: import("/wb/a").Foo; e: import("/wb/a").Foo; f: import("/wb/a").Foo; g: import("/wb/a").Foo; }' is not assignable to type '{ a: import("/wb/b").Foo; b: import("/wb/b").Foo; c: import("/wb/b").Foo; d: import("/wb/b").Foo; e: import("/wb/b").Foo; f: import("/wb/b").Foo; g: import("/wb/b").Foo; }'.
  The types of 'a.p' are incompatible between these types.
    Type 'number' is not assignable to type 'string'."#,
    ),
];

/// The first line of the checker's report where only the operands' names are
/// under test.
const EXPECTED_HEADS: &[(&str, &str)] = &[
    (
        "g12",
        r#"(28,43): error TS2322: Type 'import("/wb/a").E' is not assignable to type 'import("/wb/b").E'."#,
    ),
    (
        "g13",
        r#"(30,43): error TS2322: Type 'import("/wb/a").S' is not assignable to type 'import("/wb/b").S'."#,
    ),
    (
        "g14",
        r#"(33,47): error TS2322: Type 'Box<import("/wb/a").Foo>' is not assignable to type 'Box<import("/wb/b").Foo>'."#,
    ),
];

fn host() -> VerterHost {
    let host = host_with(MAIN);
    for (name, source) in [("a.ts", A), ("b.ts", B)] {
        let _ = host
            .upsert(crate::types::UpsertRequest {
                canonical_id: None,
                input_id: format!("{STRICT_ROOT}/{name}"),
                source: Arc::from(source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("the fixture file serves");
    }
    host
}

/// Same-named types from two modules keep the code the checker reports for
/// them and print through their modules; declarations their modules do not
/// export have no qualified spelling and still take TS2719. Cold and warm
/// answers are the same report, in the same order.
#[test]
fn same_named_operands_report_as_the_checker_tells_them_apart() {
    let host = host();
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for (symbol, expected) in EXPECTED {
            let expected = expected.replace("/wb/", &format!("{STRICT_ROOT}/"));
            let report = checker_report(&host, STRICT_ROOT, symbol, MAIN).join("\n");
            if report != expected {
                mismatches.push(format!(
                    "{pass} {symbol}:\n  got: {report}\n  expected: {expected}"
                ));
            }
        }
        for (symbol, expected) in EXPECTED_HEADS {
            let expected = expected.replace("/wb/", &format!("{STRICT_ROOT}/"));
            let report = checker_report(&host, STRICT_ROOT, symbol, MAIN).join("\n");
            let head = report.split('\n').next().unwrap_or_default();
            if head != expected {
                mismatches.push(format!(
                    "{pass} {symbol}:\n  got: {head}\n  expected: {expected}"
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
