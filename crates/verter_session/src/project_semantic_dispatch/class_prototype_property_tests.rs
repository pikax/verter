//! The `prototype` property of a class constructor type: every class — a
//! declaration, an ambient declaration, an abstract class, a generic one, a
//! derived one and a class expression — has one, typed as the class
//! instance with `any` for each of its type parameters (TypeScript's
//! `getTypeOfPrototypeProperty`). It is a property of the constructor
//! type's surface, so `keyof`, an indexed access, a property read and a
//! relation all read it.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --declaration
//! --emitDeclarationOnly --strict` under each of `strictNullChecks` and
//! `noImplicitAny` on and off (the four settings print the same answer for
//! every probe here): `export declare const v: <probe>; export const r =
//! v;` read off the declaration emitted for `r`.

use super::checker_probe_lane_tests::{mismatches, mismatches_in, ProbeProject};

const FIXTURE: &str = "\
class D0 { p = 1 as const; private q = 2; protected r = 3; static s = 4 }
class G<T> { t!: T; static gs = 1 }
abstract class A0 { abstract a: 1; static as = 2 }
class D1 extends D0 { d1 = 1 as const }
class Empty {}
declare class SP3 { static prototype: 'lit'; x: 1 }
function mk() { return class { ce = 1 as const; static z = 2 }; }
function mkg() { return class<T> { v!: T; static w = 3 }; }
class Q0 { p = 1 as const; private q = 2; protected r = 3 }
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? 'y' : 'n') : 'n';
";

/// `keyof` a class constructor type holds `prototype` beside the statics.
///
/// Measured on TypeScript 7.0.2: `keyof typeof D0` is `"prototype" | "s"`,
/// `keyof typeof G` `"gs" | "prototype"`, `keyof typeof A0` `"as" |
/// "prototype"`, `keyof typeof D1` `"prototype" | "s"`, `keyof typeof
/// Empty` and `keyof typeof SP3` `"prototype"`, `keyof ReturnType<typeof
/// mk>` `"prototype" | "z"` and `keyof ReturnType<typeof mkg>`
/// `"prototype" | "w"`.
#[test]
fn keyof_a_class_constructor_type_holds_prototype() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("keyof typeof D0", "\"prototype\" | \"s\""),
            ("keyof typeof G", "\"gs\" | \"prototype\""),
            ("keyof typeof A0", "\"as\" | \"prototype\""),
            ("keyof typeof D1", "\"prototype\" | \"s\""),
            ("keyof typeof Empty", "\"prototype\""),
            ("keyof typeof SP3", "\"prototype\""),
            ("keyof ReturnType<typeof mk>", "\"prototype\" | \"z\""),
            ("keyof ReturnType<typeof mkg>", "\"prototype\" | \"w\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A property read of `prototype` reads the instance with `any` for each
/// type parameter, and so does an indexed access; a declared static
/// `prototype` (TS2699 in a class, accepted in an ambient one) does not
/// change it.
///
/// Measured on TypeScript 7.0.2: `typeof D0.prototype` and `(typeof
/// D0)['prototype']` are `D0`, `typeof G.prototype` and `(typeof
/// G)['prototype']` `G<any>`, `(typeof SP3)['prototype']` `SP3`, `(typeof D0)['prototype']['p']` and `typeof
/// D0.prototype.p` `1`, `ReturnType<typeof mk>['prototype']['ce']` `1` and
/// `ReturnType<typeof mkg>['prototype']['v']` `any`.
#[test]
fn prototype_reads_the_instance_with_any_type_arguments() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("typeof D0.prototype", "D0"),
            ("(typeof D0)['prototype']", "D0"),
            ("(typeof G)['prototype']", "G<any>"),
            ("(typeof SP3)['prototype']", "SP3"),
            ("typeof G.prototype", "G<any>"),
            ("(typeof D0)['prototype']['p']", "1"),
            ("typeof D0.prototype.p", "1"),
            ("ReturnType<typeof mk>['prototype']['ce']", "1"),
            ("ReturnType<typeof mkg>['prototype']['v']", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An indexed access of `prototype` is the instance type: it relates both
/// ways with the class it reads, and not with its base or a structural
/// twin.
///
/// Measured on TypeScript 7.0.2, as `Same<A, B>` (`[A] extends [B]` and
/// `[B] extends [A]`): `(typeof D0)['prototype']` and `D0`, `(typeof
/// G)['prototype']` and `G<any>`, `(typeof G)['prototype']` and
/// `G<string>`, `(typeof A0)['prototype']` and `A0`, `(typeof
/// D1)['prototype']` and `D1`, `(typeof SP3)['prototype']` and `SP3`,
/// `(typeof D0)['prototype' | 's']` and `number | D0` are `"y"`; `(typeof
/// D1)['prototype']` and `D0`, `(typeof D0)['prototype']` and `Q0` (the same
/// shape declared again, whose private member is its own) and `(typeof
/// SP3)['prototype']` and `'lit'` are `"n"`.
#[test]
fn an_indexed_prototype_read_is_the_instance_type() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Same<(typeof D0)['prototype'], D0>", "\"y\""),
            ("Same<(typeof G)['prototype'], G<any>>", "\"y\""),
            ("Same<(typeof G)['prototype'], G<string>>", "\"y\""),
            ("Same<(typeof A0)['prototype'], A0>", "\"y\""),
            ("Same<(typeof D1)['prototype'], D1>", "\"y\""),
            ("Same<(typeof SP3)['prototype'], SP3>", "\"y\""),
            ("Same<(typeof D0)['prototype' | 's'], number | D0>", "\"y\""),
            ("Same<(typeof D1)['prototype'], D0>", "\"n\""),
            ("Same<(typeof D0)['prototype'], Q0>", "\"n\""),
            ("Same<(typeof SP3)['prototype'], 'lit'>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A relation reads `prototype` as a property of the constructor type.
///
/// Measured on TypeScript 7.0.2: `typeof D0 extends { prototype: D0 }` is
/// `"y"`, `typeof D0 extends { prototype: D1 }` `"n"`, `typeof G extends {
/// prototype: G<string> }` `"y"` and `(typeof D0)['prototype'] extends D0`
/// `"y"`.
#[test]
fn a_relation_reads_prototype_as_a_property() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("typeof D0 extends { prototype: D0 } ? 'y' : 'n'", "\"y\""),
            ("typeof D0 extends { prototype: D1 } ? 'y' : 'n'", "\"n\""),
            (
                "typeof G extends { prototype: G<string> } ? 'y' : 'n'",
                "\"y\"",
            ),
            ("(typeof D0)['prototype'] extends D0 ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The members of the library's `Function`, as `lib.es5.d.ts` declares
/// them.
const FUNCTION_LIB: &str = "\
interface Function { prototype: any; readonly length: number; readonly name: string; }
";

const CONSTRUCTOR_TYPES: &str = "\
interface Foo { f: 1 }
type Ctor = { new (): Foo };
type CtorFn = new () => Foo;
type CallOnly = { (): 1 };
type Both = { new (): Foo; (): 1 };
type CtorS = { new (): Foo; s: 2 };
declare const ctor: Ctor;
declare const callOnly: CallOnly;
declare const both: Both;
";

/// A constructor type that is not a class has no `prototype` of its own:
/// it reads the apparent `Function`'s, as a function type does, and
/// `keyof` lists none of the apparent members.
///
/// Measured on TypeScript 7.0.2: `Ctor['prototype']`,
/// `CtorFn['prototype']`, `Both['prototype']`, `CallOnly['prototype']`,
/// `CtorS['prototype']`, `typeof ctor.prototype`, `typeof
/// callOnly.prototype` and `typeof both.prototype` are `any`;
/// `Ctor['length']`, `CallOnly['length']` and `typeof ctor.length` are
/// `number` and `Ctor['name']` is `string`; `keyof Ctor`, `keyof CallOnly`
/// and `keyof Both` are `never` and `keyof CtorS` is `"s"`.
#[test]
fn a_constructor_type_reads_the_apparent_function_members() {
    let project = ProbeProject {
        files: &[],
        compiler_options: None,
        ambient_lib: Some(FUNCTION_LIB),
    };
    let failures = mismatches_in(
        project,
        CONSTRUCTOR_TYPES,
        &[
            ("Ctor['prototype']", "any"),
            ("CtorFn['prototype']", "any"),
            ("Both['prototype']", "any"),
            ("CallOnly['prototype']", "any"),
            ("CtorS['prototype']", "any"),
            ("typeof ctor.prototype", "any"),
            ("typeof callOnly.prototype", "any"),
            ("typeof both.prototype", "any"),
            ("Ctor['length']", "number"),
            ("CallOnly['length']", "number"),
            ("typeof ctor.length", "number"),
            ("Ctor['name']", "string"),
            ("keyof Ctor", "never"),
            ("keyof CallOnly", "never"),
            ("keyof Both", "never"),
            ("keyof CtorS", "\"s\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
