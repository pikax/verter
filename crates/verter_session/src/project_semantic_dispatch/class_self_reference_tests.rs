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

const LITERAL_GETTER: &str = "\
export const og = { n: 1, get self() { return this; } };
export function useOg() { return og.self.n; }
export function useOgDeep() { return og.self.self.n; }
export function getterFn() { return { n: 'a' as string, get self() { return this; } }; }
";

/// An object literal's getter returning its own `this` reads as the
/// literal, as a method returning `this` does.
///
/// Measured on TypeScript 7.0.2 (all four settings under `strict`):
/// `typeof og.self.n` is `number`, `typeof og extends typeof og.self` is
/// `1`, `ReturnType<typeof useOg>` and `ReturnType<typeof useOgDeep>` are
/// `number`; over `getterFn`, the return's `self`'s `n` and its `self`'s
/// `self`'s `n` are `string`, and the return's `self` extends the return
/// (`1`).
#[test]
fn a_literal_getter_returning_its_this_reads_the_literal() {
    let failures = mismatches(
        LITERAL_GETTER,
        &[
            ("typeof og.self.n", "number"),
            ("typeof og extends typeof og.self ? 1 : 0", "1"),
            ("ReturnType<typeof useOg>", "number"),
            ("ReturnType<typeof useOgDeep>", "number"),
            ("ReturnType<typeof getterFn>['self']['n']", "string"),
            ("ReturnType<typeof getterFn>['self']['self']['n']", "string"),
            (
                "ReturnType<typeof getterFn>['self'] extends ReturnType<typeof getterFn> ? 1 : 0",
                "1",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An object literal's getter is a PROPERTY of its return type on the
/// literal's whole type, never a method.
///
/// Measured on TypeScript 7.0.2 (all four settings): over `const q = { get
/// g() { return 1; } }`, `typeof q extends { g: number }` is `1`; over
/// `og`, `typeof og extends { self: Function }` is `0` and `typeof og.self
/// extends typeof og` is `1`. The lane reads the getter as a method on the
/// whole type (`{ g: () => number }`), so the first two answer `0` and `1`
/// and the last stays an unreduced conditional.
#[test]
#[ignore = "an object literal's getter is a property of its return type on the literal's whole type"]
fn a_literal_getter_is_a_property_of_the_whole_literal() {
    let source = "\
export const q = { get g() { return 1; } };
export const og = { n: 1, get self() { return this; } };
";
    let failures = mismatches(
        source,
        &[
            ("typeof q extends { g: number } ? 1 : 0", "1"),
            ("typeof og extends { self: Function } ? 1 : 0", "0"),
            ("typeof og.self extends typeof og ? 1 : 0", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without `noImplicitThis` an object literal's method or getter reads
/// `this` as `any`.
///
/// Measured on TypeScript 7.0.2 with `--strict --noImplicitThis false`
/// (all four `strictNullChecks` × `noImplicitAny` settings):
/// `ReturnType<typeof own.me>` and `typeof og.self` are `any` (`0 extends 1
/// & T` holds). The lane gives the literal under every `noImplicitThis`
/// setting.
#[test]
#[ignore = "without `noImplicitThis` an object literal's `this` is `any`"]
fn a_literal_this_is_any_without_no_implicit_this() {
    let source = "\
export const own = { v: 1, me() { return this; } };
export const og = { n: 1, get self() { return this; } };
";
    let failures = super::checker_probe_lane_tests::mismatches_in(
        super::checker_probe_lane_tests::ProbeProject {
            files: &[],
            compiler_options: Some(r#"{ "strict": true, "noImplicitThis": false }"#),
            ambient_lib: None,
        },
        source,
        &[
            ("ReturnType<typeof own.me>", "any"),
            ("typeof og.self", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class expression's members read through the value holding it: a
/// top-level `const` initializer, an instance property initializer read
/// through an instantiation of its class, and a static property
/// initializer.
///
/// Measured on TypeScript 7.0.2 (all four settings):
/// `ReturnType<InstanceType<typeof K>['m']>` is `number`, `…['s']>` is
/// `string`, `ReturnType<typeof K.st>` is `number`, and
/// `ReturnType<ReturnType<InstanceType<typeof K>['me']>['m']>` is
/// `number`; `InstanceType<Outer2<string>['inner']>['u']` is `string`,
/// `ReturnType<InstanceType<Outer2<string>['inner']>['g']>` is `number`,
/// and `ReturnType<InstanceType<typeof Outer2.sinner>['m']>` is `number`.
/// The lane answers every row with the missing-member marker.
#[test]
#[ignore = "a class expression held by a `const` or a property initializer serves its members"]
fn a_class_expression_initializer_serves_its_members() {
    let source = "\
export class Outer2<U> { inner = class { u!: U; me() { return this; } g() { return 1; } }; static sinner = class { w = 1; m() { return this.w; } }; }
export const K = class { v = 1; m() { return this.v; } s() { return 'a' as string; } me() { return this; } static st() { return 2; } };
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<InstanceType<typeof K>['m']>", "number"),
            ("ReturnType<InstanceType<typeof K>['s']>", "string"),
            ("ReturnType<typeof K.st>", "number"),
            (
                "ReturnType<ReturnType<InstanceType<typeof K>['me']>['m']>",
                "number",
            ),
            ("InstanceType<Outer2<string>['inner']>['u']", "string"),
            (
                "ReturnType<InstanceType<Outer2<string>['inner']>['g']>",
                "number",
            ),
            (
                "ReturnType<InstanceType<typeof Outer2.sinner>['m']>",
                "number",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A method called directly on a `new` expression or on a call's result
/// reads the method through the constructed or returned value, and a
/// method reading `this.v` or calling `this.m()` reads the receiver's
/// instantiation.
///
/// Measured on TypeScript 7.0.2 (all four settings): `ReturnType<typeof
/// …>` of `n1`, `n2`, `g1`, `g6` and `useOwn` is `number`, of `g2`
/// `string`, and `ReturnType<typeof g3>['v']` is `boolean`. The lane reads
/// the member call on such a receiver as an unmodelled position (the same
/// methods called through a `const` or a parameter of the class type
/// already read `number` and `string`).
#[test]
#[ignore = "a method called on a `new` expression or a call's result reads the receiver's member"]
fn a_method_called_on_a_constructed_value_reads_the_receiver() {
    let source = "\
export class G<T> { constructor(public v: T) {} get() { return this.v; } call() { return this.get(); } me() { return this; } }
export class N { v = 1; get() { return this.v; } call() { return this.get(); } }
export class GF<T> { f!: T; get() { return this.f; } }
export function n1() { return new N().get(); }
export function n2() { return new N().call(); }
export function g1() { return new G(1).get(); }
export function g2() { return new G('s').call(); }
export function g3() { return new G(true).me(); }
export function g6() { return new GF<number>().get(); }
export const own = { v: 1, me() { return this; } };
export function useOwn() { return own.me().v; }
";
    let failures = mismatches(
        source,
        &[
            ("ReturnType<typeof n1>", "number"),
            ("ReturnType<typeof n2>", "number"),
            ("ReturnType<typeof g1>", "number"),
            ("ReturnType<typeof g2>", "string"),
            ("ReturnType<typeof g3>['v']", "boolean"),
            ("ReturnType<typeof g6>", "number"),
            ("ReturnType<typeof useOwn>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
