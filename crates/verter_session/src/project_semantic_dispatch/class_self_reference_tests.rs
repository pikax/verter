//! A value's members read one at a time: a member whose type names the
//! value it belongs to — a static method returning its class, a method
//! reading its class through a parameter of the class's type, an object
//! literal's method reading the variable that holds it or returning its
//! own `this` — reads the member it names where the value declares it,
//! never the whole surface it is itself part of.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`), under all four
//! `strictNullChecks` × `noImplicitAny` settings; every answer below is the
//! same under all four.

use super::checker_probe_lane_tests::mismatches;

const STATICS: &str = "\
export class S { static k = 1; static s() { return this; } static e() { return S; } static kk() { return S.k; } static viaS() { return this.s(); } }
export class B0s { static bk = 1; static bm() { return 'b' as string; } }
export class S2 extends B0s { static r() { return S2.bk; } static q() { return S2.bm(); } static w() { return this.bk; } static me() { return S2; } static ann: number = 1; static y() { return this.ann; } static x() { return this.bm(); } }
export class G2<T> { t!: T; static k = 1; static r() { return G2.k; } }
";

/// A static method returning its class — as `this`, by name, or through a
/// sibling returning `this` — returns the class's constructor type, read
/// by reference; a static reading its class's own or inherited static by
/// name or through `this` reads that static alone.
///
/// Measured on TypeScript 7.0.2 (all four `strictNullChecks` ×
/// `noImplicitAny` settings): `ReturnType<typeof S.s>`, `ReturnType<typeof
/// S.e>` and `ReturnType<typeof S.viaS>` are `typeof S`, and `typeof S`
/// extends the first two and they extend it (`1` each way);
/// `ReturnType<typeof S.s>['k']`, `ReturnType<typeof S.e>['k']` and
/// `ReturnType<typeof S.kk>` are `number`; over `S2 extends B0s`,
/// `ReturnType<typeof S2.r>`, `…w` and `…y` are `number`, `…q` and `…x`
/// are `string`, and `ReturnType<typeof S2.me>` is `typeof S2`;
/// `ReturnType<typeof G2.r>` is `number`. Read through the whole
/// constructor type, `ReturnType<(typeof S)['s']>['k']`,
/// `ReturnType<(typeof S)['e']>['k']`, `ReturnType<(typeof S)['kk']>`
/// and `ReturnType<(typeof S2)['r']>` are `number` and
/// `ReturnType<(typeof S2)['q']>` is `string`.
#[test]
fn a_static_reading_its_own_class_reads_the_member_alone() {
    let failures = mismatches(
        STATICS,
        &[
            ("ReturnType<typeof S.s> extends typeof S ? 1 : 0", "1"),
            ("typeof S extends ReturnType<typeof S.s> ? 1 : 0", "1"),
            ("ReturnType<typeof S.e> extends typeof S ? 1 : 0", "1"),
            ("typeof S extends ReturnType<typeof S.e> ? 1 : 0", "1"),
            ("ReturnType<typeof S.viaS> extends typeof S ? 1 : 0", "1"),
            ("ReturnType<typeof S.s>['k']", "number"),
            ("ReturnType<typeof S.e>['k']", "number"),
            ("ReturnType<typeof S.kk>", "number"),
            ("ReturnType<typeof S2.r>", "number"),
            ("ReturnType<typeof S2.q>", "string"),
            ("ReturnType<typeof S2.w>", "number"),
            ("ReturnType<typeof S2.y>", "number"),
            ("ReturnType<typeof S2.x>", "string"),
            ("ReturnType<typeof S2.me> extends typeof S2 ? 1 : 0", "1"),
            ("ReturnType<typeof G2.r>", "number"),
            ("ReturnType<(typeof S)['s']>['k']", "number"),
            ("ReturnType<(typeof S)['e']>['k']", "number"),
            ("ReturnType<(typeof S)['kk']>", "number"),
            ("ReturnType<(typeof S2)['r']>", "number"),
            ("ReturnType<(typeof S2)['q']>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const THROUGH_PARAMETERS: &str = "\
export class H2 { v = 1; m(h: H2) { return h.v; } n(h: H2) { return h.m(h); } }
export class H3 { v = 1; m(h: H3) { const o: H3 = h; return o.v; } k(h: H3) { return h.m(h); } }
";

/// A method reading its own class through a parameter of the class's type
/// (directly, through a local of that type, or by calling a sibling with
/// the parameter) reads the member where the class declares it.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<H2['m']>`,
/// `ReturnType<H2['n']>`, `ReturnType<H3['m']>` and `ReturnType<H3['k']>`
/// are `number`.
#[test]
fn a_method_reading_its_class_through_a_parameter_reads_the_member() {
    let failures = mismatches(
        THROUGH_PARAMETERS,
        &[
            ("ReturnType<H2['m']>", "number"),
            ("ReturnType<H2['n']>", "number"),
            ("ReturnType<H3['m']>", "number"),
            ("ReturnType<H3['k']>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERAL_BY_NAME: &str = "\
export const obj = { v: 1, m() { return obj.v; }, n() { return obj.m(); } };
export const o2 = { a: { b: 1 }, m() { return o2.a.b; }, n() { return o2.m(); }, self() { return o2; } };
";

/// An object literal's method reading the variable that holds the literal
/// reads the member it names where the literal declares it.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof
/// obj.m>`, `ReturnType<typeof obj.n>`, `ReturnType<typeof o2.m>`,
/// `ReturnType<typeof o2.n>`, `ReturnType<typeof o2.self>['a']['b']` and,
/// through the whole literal, `ReturnType<(typeof obj)['m']>` are `number`.
#[test]
fn a_literal_method_reading_its_variable_reads_the_member() {
    let failures = mismatches(
        LITERAL_BY_NAME,
        &[
            ("ReturnType<typeof obj.m>", "number"),
            ("ReturnType<typeof obj.n>", "number"),
            ("ReturnType<typeof o2.m>", "number"),
            ("ReturnType<typeof o2.n>", "number"),
            ("ReturnType<typeof o2.self>['a']['b']", "number"),
            ("ReturnType<(typeof obj)['m']>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERAL_THIS: &str = "\
export const own = { v: 1, me() { return this; } };
export function selfFn() { return { v: 1, me() { return this; } }; }
export function viaLocal() { const o = { w: 'a' as string, me() { return this; } }; return o; }
";

/// An object literal whose method returns its own `this` has the checker's
/// recursive anonymous type, which prints `{ v: number; me(): ...; }`: a
/// variable's literal is named by `typeof own`, and a literal a function
/// returns is its own identity. Either way the type behaves as the
/// checker's one type does — its members read, `me` returns the literal
/// again, and it relates to itself both ways but not to a one-level copy.
/// The prints differ (the checker elides the recursion; this lane names
/// `typeof own`, or prints the literal with `this` at the recursion),
/// which the ledger records as presentation only; these rows are the
/// effect equality.
///
/// Measured on TypeScript 7.0.2 (all four settings under `strict`; with
/// `noImplicitThis` off a literal's `this` is `any` instead):
/// `ReturnType<typeof own.me>['v']` and `ReturnType<ReturnType<typeof
/// own.me>['me']>['v']` and, through the whole literal,
/// `ReturnType<(typeof own)['me']>['v']` are `number`, and `typeof own
/// extends ReturnType<typeof own.me>` is `1`; over `selfFn` the return is `{ v:
/// number; me(): ...; }`, the `v` of the return, of `me`'s return and of
/// `me`'s return's `me`'s return are `number`, the return and `me`'s
/// return extend each other (`1`, `1`), the return extends `{ v: number
/// }` (`1`), and `{ v: number; me(): { v: number } }` does not extend the
/// return (`0`); over `viaLocal` the return is `{ w: string; me(): ...;
/// }`, the `w` of `me`'s return is `string`, and the return extends `me`'s
/// return (`1`).
#[test]
fn a_literal_returning_its_this_is_one_recursive_type() {
    let failures = mismatches(
        LITERAL_THIS,
        &[
            ("ReturnType<typeof own.me>['v']", "number"),
            ("ReturnType<ReturnType<typeof own.me>['me']>['v']", "number"),
            ("typeof own extends ReturnType<typeof own.me> ? 1 : 0", "1"),
            ("ReturnType<(typeof own)['me']>['v']", "number"),
            ("ReturnType<typeof selfFn>['v']", "number"),
            ("ReturnType<ReturnType<typeof selfFn>['me']>['v']", "number"),
            (
                "ReturnType<ReturnType<ReturnType<typeof selfFn>['me']>['me']>['v']",
                "number",
            ),
            (
                "ReturnType<typeof selfFn> extends ReturnType<ReturnType<typeof selfFn>['me']> ? 1 : 0",
                "1",
            ),
            (
                "ReturnType<ReturnType<typeof selfFn>['me']> extends ReturnType<typeof selfFn> ? 1 : 0",
                "1",
            ),
            ("ReturnType<typeof selfFn> extends { v: number } ? 1 : 0", "1"),
            (
                "{ v: number; me(): { v: number } } extends ReturnType<typeof selfFn> ? 1 : 0",
                "0",
            ),
            ("ReturnType<ReturnType<typeof viaLocal>['me']>['w']", "string"),
            (
                "ReturnType<typeof viaLocal> extends ReturnType<ReturnType<typeof viaLocal>['me']> ? 1 : 0",
                "1",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
