//! A generic class's body-derived member returns under an instantiated
//! receiver. The body reads the class's own type parameter; the receiver's
//! type arguments reach the member's return exactly as they reach a
//! declared one (`GH<number>`'s `get()` returns `number`, not `H`), through
//! a direct member read, a call on a value of the instantiated type, and an
//! inherited member of a class that extends an instantiation. A method's
//! own type parameter shadows the class's.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `declare const v: <probe>; export const s: null = v;` read
//! off the TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export class GH<H> { get() { return null as unknown as H; } obj() { return { h: null as unknown as H }; } declared(): H { return null!; } arrow = () => null as unknown as H; }
export class GS<H> { m<H>(x: H) { return x; } k(x: H) { return [x]; } }
export class Sub extends GH<boolean> {}
export declare const gh: GH<number>;
export declare const gs: GS<number>;
export function callGet() { return gh.get(); }
export function callObj() { return gh.obj(); }
export function callShadow() { return gs.m('s'); }
export function callK() { return gs.k(1); }
";

/// A member read off an instantiated class returns the receiver's type
/// argument where the body returns the class's parameter.
///
/// Measured on TypeScript 7.0.2: `ReturnType<GH<number>['get']>` is
/// `number`, `ReturnType<GH<number>['obj']>` is `{ h: number; }`,
/// `ReturnType<GH<number>['arrow']>` is `number`, `ReturnType<GH<number>
/// ['declared']>` is `number`, `ReturnType<GS<number>['k']>` is
/// `number[]` and the inherited `ReturnType<Sub['get']>` is `boolean`.
#[test]
fn a_body_derived_member_return_reads_the_receiver_type_arguments() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<GH<number>['get']>", "number"),
            ("ReturnType<GH<number>['obj']>", "{ h: number; }"),
            ("ReturnType<GH<number>['arrow']>", "number"),
            ("ReturnType<GH<number>['declared']>", "number"),
            ("ReturnType<GS<number>['k']>", "number[]"),
            ("ReturnType<Sub['get']>", "boolean"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A call on a value of the instantiated type returns the same, and a
/// method's own type parameter shadows the class's.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof callGet>` is `number`,
/// `ReturnType<typeof callObj>` is `{ h: number; }`, `ReturnType<typeof
/// callK>` is `number[]` and `ReturnType<typeof callShadow>` is `string`.
#[test]
fn a_call_on_an_instantiated_receiver_returns_its_type_arguments() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof callGet>", "number"),
            ("ReturnType<typeof callObj>", "{ h: number; }"),
            ("ReturnType<typeof callK>", "number[]"),
            ("ReturnType<typeof callShadow>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
