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
export interface Bar { a: number }
export interface Multi { x: number }
export interface Many { x: number }
export interface Deep { n: Bar }
interface Thin { a: number }
export declare const thin: Thin;
export enum Gone { X, Y }
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
export interface Bar { a: number; b: string }
export interface Multi { x: number; y: number; z: number }
export interface Many { x: number; p1: number; p2: number; p3: number; p4: number; p5: number; p6: number }
export interface Deep { n: Bar }
interface Thin { a: number; b: string }
export declare function takeThin(x: { q: Thin }): void;
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
import { Gone, thin } from './a';
import type { Bar as BarA, Multi as MultiA, Many as ManyA, Deep as DeepA } from './a';
import { takeThin } from './b';
import type { Bar as BarB, Multi as MultiB, Many as ManyB, Deep as DeepB } from './b';
declare function f17(x: { q: BarB }): void;
export function g17(a: BarA) { return f17({ q: a }); }
declare function f18(x: BarB): void;
export function g18(a: BarA) { return f18(a); }
declare function f19(x: { q: MultiB }): void;
export function g19(a: MultiA) { return f19({ q: a }); }
declare function f20(x: { q: ManyB }): void;
export function g20(a: ManyA) { return f20({ q: a }); }
declare function f21(x: { q: DeepB }): void;
export function g21(a: DeepA) { return f21({ q: a }); }
declare function f22(x: { q: { n: MultiB } }): void;
export function g22(a: { n: MultiA }) { return f22({ q: a }); }
declare function f23(x: { q: MultiB[] }): void;
export function g23(a: MultiA[]) { return f23({ q: a }); }
export function g24() { return takeThin({ q: thin }); }
declare function f25(x: string): void;
export function g25() { return f25(Gone.X); }
declare function f26(x: { q: string }): void;
export function g26() { return f26({ q: Gone.X }); }
export function g27(a: Gone) { return f25(a); }
enum Kept { A, B }
export function g28() { return f25(Kept.A); }
declare function f29(x: boolean): void;
export function g29(a: Gone.X | "s") { return f29(a); }
type Lits = "a" | "b";
declare function f30(x: number): void;
export function g30(a: Lits) { return f30(a); }
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
    (
        "g17",
        r#"(41,45): error TS2741: Property 'b' is missing in type 'import("/wb/a").Bar' but required in type 'import("/wb/b").Bar'."#,
    ),
    (
        "g18",
        r#"(43,43): error TS2741: Property 'b' is missing in type 'import("/wb/a").Bar' but required in type 'import("/wb/b").Bar'."#,
    ),
    (
        "g19",
        r#"(45,47): error TS2739: Type 'import("/wb/a").Multi' is missing the following properties from type 'import("/wb/b").Multi': y, z"#,
    ),
    (
        "g20",
        r#"(47,46): error TS2740: Type 'import("/wb/a").Many' is missing the following properties from type 'import("/wb/b").Many': p1, p2, p3, p4, and 2 more."#,
    ),
    (
        "g21",
        r#"(49,46): error TS2322: Type 'import("/wb/a").Deep' is not assignable to type 'import("/wb/b").Deep'.
  Types of property 'n' are incompatible.
    Property 'b' is missing in type 'import("/wb/a").Bar' but required in type 'import("/wb/b").Bar'."#,
    ),
    (
        "g22",
        r#"(51,54): error TS2322: Type '{ n: import("/wb/a").Multi; }' is not assignable to type '{ n: import("/wb/b").Multi; }'.
  Types of property 'n' are incompatible.
    Type 'import("/wb/a").Multi' is missing the following properties from type 'import("/wb/b").Multi': y, z"#,
    ),
    (
        "g23",
        r#"(53,49): error TS2322: Type 'import("/wb/a").Multi[]' is not assignable to type 'import("/wb/b").Multi[]'.
  Type 'import("/wb/a").Multi' is missing the following properties from type 'import("/wb/b").Multi': y, z"#,
    ),
    (
        "g24",
        r#"(54,43): error TS2741: Property 'b' is missing in type 'Thin' but required in type 'Thin'."#,
    ),
    (
        "g25",
        r#"(56,36): error TS2345: Argument of type 'import("/wb/a").Gone' is not assignable to parameter of type 'string'."#,
    ),
    (
        "g26",
        r#"(58,38): error TS2322: Type 'import("/wb/a").Gone' is not assignable to type 'string'."#,
    ),
    (
        "g27",
        r#"(59,43): error TS2345: Argument of type 'import("/wb/a").Gone' is not assignable to parameter of type 'string'."#,
    ),
    (
        "g28",
        r#"(61,36): error TS2345: Argument of type 'Kept' is not assignable to parameter of type 'string'."#,
    ),
    (
        "g29",
        r#"(63,51): error TS2345: Argument of type 'string | import("/wb/a").Gone' is not assignable to parameter of type 'boolean'.
  Type 'string' is not assignable to type 'boolean'."#,
    ),
    (
        "g30",
        r#"(66,43): error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'.
  Type 'string' is not assignable to type 'number'."#,
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

const LEFT: &str = r#"interface Eq { p: number }
export = Eq;
"#;

const RIGHT: &str = r#"interface Eq { p: string }
export = Eq;
"#;

const LEFT_NS: &str = r#"namespace Eq { export interface Inner { p: number } }
export = Eq;
"#;

const RIGHT_NS: &str = r#"namespace Eq { export interface Inner { p: string } }
export = Eq;
"#;

const ASSIGNED_MAIN: &str = r#"import type Left from './left';
import type Right from './right';
import LeftNs = require('./left-ns');
import RightNs = require('./right-ns');
declare function f1(x: { q: Right }): void;
export function g1(a: Left) { return f1({ q: a }); }
declare function f2(x: Right): void;
export function g2(a: Left) { return f2(a); }
declare function f3(x: { q: RightNs.Inner }): void;
export function g3(a: LeftNs.Inner) { return f3({ q: a }); }
"#;

/// The checker's report for each function of [`ASSIGNED_MAIN`], rooted at
/// `/wb` and measured with `--module commonjs`, which `export =` requires.
const ASSIGNED_EXPECTED: &[(&str, &str)] = &[
    (
        "g1",
        r#"(6,43): error TS2322: Type 'import("/wb/left")' is not assignable to type 'import("/wb/right")'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g2",
        r#"(8,41): error TS2345: Argument of type 'import("/wb/left")' is not assignable to parameter of type 'import("/wb/right")'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
    (
        "g3",
        r#"(10,51): error TS2322: Type 'import("/wb/left-ns").Inner' is not assignable to type 'import("/wb/right-ns").Inner'.
  Types of property 'p' are incompatible.
    Type 'number' is not assignable to type 'string'."#,
    ),
];

fn host_of(main: &str, files: &[(&str, &str)]) -> VerterHost {
    let host = host_with(main);
    for (name, source) in files {
        let _ = host
            .upsert(crate::types::UpsertRequest {
                canonical_id: None,
                input_id: format!("{STRICT_ROOT}/{name}"),
                source: Arc::from(*source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("the fixture file serves");
    }
    host
}

fn host() -> VerterHost {
    host_of(MAIN, &[("a.ts", A), ("b.ts", B)])
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

/// `export =` makes a declaration its module: it prints as the module, and a
/// namespace member as a path below it, so two same-named declarations stay
/// told apart and keep TS2322 where a bare print would take TS2719.
#[test]
fn an_assigned_export_prints_as_its_module() {
    let host = host_of(
        ASSIGNED_MAIN,
        &[
            ("left.ts", LEFT),
            ("right.ts", RIGHT),
            ("left-ns.ts", LEFT_NS),
            ("right-ns.ts", RIGHT_NS),
        ],
    );
    let mut mismatches = Vec::new();
    for pass in ["cold", "warm"] {
        for (symbol, expected) in ASSIGNED_EXPECTED {
            let expected = expected.replace("/wb/", &format!("{STRICT_ROOT}/"));
            let report = checker_report(&host, STRICT_ROOT, symbol, ASSIGNED_MAIN).join("\n");
            if report != expected {
                mismatches.push(format!(
                    "{pass} {symbol}:\n  got: {report}\n  expected: {expected}"
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
