//! Relations between two signatures: the checker's
//! `compareSignaturesRelated` read through the one positional model. A
//! rest parameter supplies its element at every position past the fixed
//! ones, an `any[]` rest (or an `any` rest) accepts any source arity, and a
//! fixed tuple rest compares element by element.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture: `declare const v: <probe>; export const s: null = v;`
//! read off the TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export class N2 { n = 1; constructor(a: string) {} }
export class Named { n = 1; constructor(a: string, b?: number) {} static s = 's'; }
declare function mixin<T extends new (...args: any[]) => {}>(b: T): T & (new (...args: any[]) => { mixed: boolean });
declare function keep<T extends new (...args: any[]) => {}>(b: T): T;
declare function wrap<T extends (...args: any[]) => void>(f: T): T;
declare function take(x: string): void;
export function mixinCall() { return mixin(N2); }
export function keepCall() { return keep(Named); }
export function wrapCall() { return wrap(take); }
";

/// A fixed-arity source relates to a rest target position by position
/// against the rest's element: `any[]` and `any` rests accept every
/// source, `never[]` accepts every source whose parameters `never` is
/// assignable to, and an element type the source parameter does not
/// accept rejects.
///
/// Measured on TypeScript 7.0.2: `((a: string) => void) extends ((...args:
/// any[]) => void)` is `1`, and so are the construct form `(new (a:
/// string) => {}) extends (new (...args: any[]) => {})`, `(new (a: string)
/// => N2) extends (new (...args: any[]) => N2)` and `typeof N2 extends new
/// (...args: any[]) => {}`; `((a: string, b: number) => void) extends
/// ((...args: string[]) => void)` is `0`; `((...a: string[]) => void)
/// extends ((x: string, y: string) => void)` is `1`; `((a: string) =>
/// void) extends ((...args: unknown[]) => void)` is `0`; `((a: string) =>
/// void) extends ((...args: never[]) => void)` is `1`; `((a: string, b?:
/// number) => void) extends ((...args: any) => void)` is `1`.
#[test]
fn a_rest_target_supplies_its_element_at_every_source_position() {
    let failures = mismatches(
        FIXTURE,
        &[
            (
                "((a: string) => void) extends ((...args: any[]) => void) ? 1 : 0",
                "1",
            ),
            (
                "(new (a: string) => {}) extends (new (...args: any[]) => {}) ? 1 : 0",
                "1",
            ),
            (
                "(new (a: string) => N2) extends (new (...args: any[]) => N2) ? 1 : 0",
                "1",
            ),
            (
                "(typeof N2) extends (new (...args: any[]) => {}) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b: number) => void) extends ((...args: string[]) => void) ? 1 : 0",
                "0",
            ),
            (
                "((...a: string[]) => void) extends ((x: string, y: string) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string) => void) extends ((...args: unknown[]) => void) ? 1 : 0",
                "0",
            ),
            (
                "((a: string) => void) extends ((...args: never[]) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b?: number) => void) extends ((...args: any) => void) ? 1 : 0",
                "1",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The arity verdict: a source that needs more arguments than a rest-less
/// target supplies rejects, a target with a rest supplies any arity, and a
/// fixed tuple rest is its elements.
///
/// Measured on TypeScript 7.0.2: `((a: any, b: any) => void) extends ((a?:
/// any, b?: any) => void)` is `1` but `extends ((a?: any) => void)` is `0`;
/// `((a: string, ...r: number[]) => void)` extends `((a: string, b: number,
/// c: number) => void)` (`1`) and not `((a: string, b: string) => void)`
/// (`0`); `((...r: [string, number]) => void)` and `((a: string, b: number)
/// => void)` extend each other (`1`, `1`); `((a: string, b: number, c:
/// boolean) => void) extends ((...r: [string, number]) => void)` is `0`;
/// `((a: string) => void) extends ((a: string, ...r: any[]) => void)` is
/// `1`, as are `((a: string, b: number) => void) extends ((...r: any) =>
/// void)`, `(() => void) extends ((...r: any[]) => void)` and `((...r:
/// any[]) => void) extends ((a: number) => void)`; `((a: number, b?:
/// string) => void) extends ((...r: [number]) => void)` is `1`;
/// `((a: string, b: number) => void) extends ((a: string) => void)` is `0`.
#[test]
fn the_arity_verdict_reads_the_target_rest() {
    let failures = mismatches(
        FIXTURE,
        &[
            (
                "((a: any, b: any) => void) extends ((a?: any, b?: any) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: any, b: any) => void) extends ((a?: any) => void) ? 1 : 0",
                "0",
            ),
            (
                "((a: string, ...r: number[]) => void) extends ((a: string, b: number, c: number) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, ...r: number[]) => void) extends ((a: string, b: string) => void) ? 1 : 0",
                "0",
            ),
            (
                "((...r: [string, number]) => void) extends ((a: string, b: number) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b: number) => void) extends ((...r: [string, number]) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b: number, c: boolean) => void) extends ((...r: [string, number]) => void) ? 1 : 0",
                "0",
            ),
            (
                "((a: string) => void) extends ((a: string, ...r: any[]) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b: number) => void) extends ((...r: any) => void) ? 1 : 0",
                "1",
            ),
            ("(() => void) extends ((...r: any[]) => void) ? 1 : 0", "1"),
            (
                "((...r: any[]) => void) extends ((a: number) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: number, b?: string) => void) extends ((...r: [number]) => void) ? 1 : 0",
                "1",
            ),
            (
                "((a: string, b: number) => void) extends ((a: string) => void) ? 1 : 0",
                "0",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A call whose type parameter is constrained by an `any[]`-rest signature
/// infers from an argument with fixed parameters: the argument satisfies
/// the constraint, so the call returns the argument's own type.
///
/// Measured on TypeScript 7.0.2: `InstanceType<ReturnType<typeof
/// mixinCall>>` is `N2 & { mixed: boolean; }`, `InstanceType<ReturnType<
/// typeof keepCall>>` is `Named` and `ReturnType<typeof wrapCall>` is
/// `(x: string) => void`.
#[test]
fn a_rest_constrained_call_accepts_a_fixed_parameter_argument() {
    let failures = mismatches(
        FIXTURE,
        &[
            (
                "InstanceType<ReturnType<typeof mixinCall>>",
                "N2 & { mixed: boolean; }",
            ),
            ("InstanceType<ReturnType<typeof keepCall>>", "Named"),
            ("ReturnType<typeof wrapCall>", "(x: string) => void"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
