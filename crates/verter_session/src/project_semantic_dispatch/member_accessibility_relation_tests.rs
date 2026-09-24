//! A relation compares the accessibility of a property pair before its
//! types, as the checker's `propertyRelatedTo` does: a `private` property
//! on either side relates only to the same declaration, a `protected`
//! target property only to one declared in a class derived from the
//! target property's declaring class, and a `protected` source property
//! never to a public one. `keyof` lists public members only.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict` under each of `strictNullChecks` and
//! `noImplicitAny` on and off (the four settings print the same answer for
//! every probe here): `export declare const v: <probe>; export const r =
//! v;` read off the declaration emitted for `r`.

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
class D0 { p = 1 as const; private q = 2; protected r = 3; static s = 4 }
class Q0 { p = 1 as const; private q = 2; protected r = 3 }
class PV { private x = 1 }
class PV2 { private x = 1 }
class PR { protected x = 1 }
class PR2 { protected x = 1 }
class PB { x = 1 }
class Sub extends PV { y = 2 }
class SubPR extends PR { y = 2 }
class SubPR2 extends PR { protected x = 2 }
class SubPub extends PR { x = 3 }
class Grand extends SubPR2 {}
class G<T> { private v!: T }
interface IPub { x: number }
type PubX = { x: number };
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? 'y' : 'n') : 'n';
type Ext<A, B> = [A] extends [B] ? 'y' : 'n';
";

/// A private property relates only to the same declaration, in either
/// direction and against a public one.
///
/// Measured on TypeScript 7.0.2: `Same<D0, Q0>`, `Ext<D0, Q0>`, `Ext<Q0,
/// D0>`, `Ext<PV, PV2>`, `Ext<PV, PB>`, `Ext<PB, PV>`, `Ext<PubX, PV>` and
/// `Ext<PV, PubX>` are `"n"`; `Ext<PV, PV>`, `Ext<Sub, PV>`, `Ext<PV & {
/// z: 1 }, PV>`, `Ext<PV, {}>`, `Ext<G<1>, G<1>>` and `Ext<G<1>,
/// G<number>>` are `"y"`, `Ext<PV, Sub>` and `Ext<G<1>, G<2>>` `"n"`.
#[test]
fn a_private_property_relates_only_to_its_own_declaration() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Same<D0, Q0>", "\"n\""),
            ("Ext<D0, Q0>", "\"n\""),
            ("Ext<Q0, D0>", "\"n\""),
            ("Ext<PV, PV2>", "\"n\""),
            ("Ext<PV, PB>", "\"n\""),
            ("Ext<PB, PV>", "\"n\""),
            ("Ext<PubX, PV>", "\"n\""),
            ("Ext<PV, PubX>", "\"n\""),
            ("Ext<PV, PV>", "\"y\""),
            ("Ext<Sub, PV>", "\"y\""),
            ("Ext<PV & { z: 1 }, PV>", "\"y\""),
            ("Ext<PV, {}>", "\"y\""),
            ("Ext<PV, Sub>", "\"n\""),
            ("Ext<G<1>, G<1>>", "\"y\""),
            ("Ext<G<1>, G<number>>", "\"y\""),
            ("Ext<G<1>, G<2>>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A protected target property relates to a property declared in a class
/// derived from its declaring class, the same declaration included; a
/// protected source property never relates to a public one.
///
/// Measured on TypeScript 7.0.2: `Ext<SubPR, PR>`, `Ext<SubPR2, PR>`,
/// `Ext<SubPub, PR>` and `Ext<Grand, PR>` are `"y"`; `Ext<PR, PR2>`,
/// `Ext<PB, PR>`, `Ext<PR, PB>`, `Ext<PR, SubPR2>`, `Ext<PR, SubPub>`,
/// `Ext<SubPR2, PR2>`, `Ext<IPub, PR>`, `Ext<PubX, PR>` and `Ext<PR,
/// IPub>` are `"n"`.
#[test]
fn a_protected_property_relates_only_down_its_class_hierarchy() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Ext<SubPR, PR>", "\"y\""),
            ("Ext<SubPR2, PR>", "\"y\""),
            ("Ext<SubPub, PR>", "\"y\""),
            ("Ext<Grand, PR>", "\"y\""),
            ("Ext<PR, PR2>", "\"n\""),
            ("Ext<PB, PR>", "\"n\""),
            ("Ext<PR, PB>", "\"n\""),
            ("Ext<PR, SubPR2>", "\"n\""),
            ("Ext<PR, SubPub>", "\"n\""),
            ("Ext<SubPR2, PR2>", "\"n\""),
            ("Ext<IPub, PR>", "\"n\""),
            ("Ext<PubX, PR>", "\"n\""),
            ("Ext<PR, IPub>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` a class instance lists its public members only.
///
/// Measured on TypeScript 7.0.2: `keyof D0` is `"p"` and `keyof PV`
/// `never`.
#[test]
fn keyof_lists_public_members_only() {
    let failures = mismatches(FIXTURE, &[("keyof D0", "\"p\""), ("keyof PV", "never")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
