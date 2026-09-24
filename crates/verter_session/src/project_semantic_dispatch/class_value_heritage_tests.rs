//! A class declaration that `extends` a VALUE the type space does not
//! declare (`class KE extends Ctor` over `declare const Ctor: new () => …`)
//! takes its base from that value, as TypeScript's
//! `getBaseConstructorTypeOfClass` and `resolveBaseTypesOfClass` do: the
//! instance side inherits the FIRST construct signature's return — when the
//! constructor type is an object and that return is a valid base type — and
//! the static side inherits the constructor type's members and, for a class
//! without its own constructor, its construct signatures with the derived
//! instance as their result.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit` (strict by default):
//! `declare const v: <probe>; export const s: null = v;` read off the
//! TS2322 message. The fixture also draws TS2510 (`KO`: base constructors
//! with different return types) and TS2507 (`KNC`: not a constructor
//! function type), whose answers are the checker's error recovery.

use super::checker_probe_lane_tests::mismatches;

const VALUE_BASES: &str = "\
declare const Ctor: new () => { (): 'base-call'; x: 1 };
export class KE extends Ctor { own = 'o' as const }
interface IK extends KE { (): 'own' }
export class KE2 extends KE {}
declare const CtorP: { new (a: string, b: number): { p: 'p' }; st: 'static' };
export class KP extends CtorP {}
export class KP2 extends KP {}
declare const CtorO: { new (a: string): { o: 'str' }; new (a: number): { o: 'num' } };
export class KO extends CtorO {}
declare const GCtor: new <T>(v: T) => { val: T };
export class KG extends GCtor<string> {}
declare const CtorD: new (a: string) => { d: 'd' };
export class KD extends CtorD { constructor() { super('x'); } }
declare const U: (new () => { a: 1 }) | (new () => { b: 2 });
export class KU extends U {}
declare const U3: (new (x: string) => { a: 1 }) | (new (x: number) => { a: 1 });
export class KU3 extends U3 {}
declare const NC: { x: 1 };
export class KNC extends NC {}
declare const CS: new () => string;
export class KS extends CS { own = 1 as const }
declare const CU: new () => { a: 1 } | { b: 2 };
export class KUU extends CU {}
";

/// The instance side inherits the value's construct-signature result,
/// call signature included, through a derived class and an interface that
/// extends the class; overloaded constructors give the FIRST signature's
/// result; a generic constructor is instantiated with the clause's type
/// arguments.
///
/// Measured on TypeScript 7.0.2: `KE['x']`, `InstanceType<typeof
/// KE>['x']` and `KE2['x']` are `1`, `KE['own']` is `"o"`,
/// `ReturnType<KE>`, `ReturnType<KE2>` and `ReturnType<IK>` are
/// `"base-call"`, `KP['p']` is `"p"`, `KO['o']` and `InstanceType<typeof
/// KO>['o']` are `"str"`, `KG['val']` is `string`, `KD['d']` is `"d"`, and
/// `KP extends { p: 'p' }` is `"y"`.
#[test]
fn a_value_base_gives_the_instance_its_construct_signature_result() {
    let failures = mismatches(
        VALUE_BASES,
        &[
            ("KE['x']", "1"),
            ("KE['own']", "\"o\""),
            ("InstanceType<typeof KE>['x']", "1"),
            ("KE2['x']", "1"),
            ("ReturnType<KE>", "\"base-call\""),
            ("ReturnType<KE2>", "\"base-call\""),
            ("ReturnType<IK>", "\"base-call\""),
            ("KP['p']", "\"p\""),
            ("KP extends { p: 'p' } ? 'y' : 'n'", "\"y\""),
            ("KO['o']", "\"str\""),
            ("InstanceType<typeof KO>['o']", "\"str\""),
            ("KG['val']", "string"),
            ("KD['d']", "\"d\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The static side inherits the value's members and, for a class with no
/// constructor of its own, every construct signature — instantiated with
/// the clause's type arguments — through a derived class too; a declared
/// constructor replaces them.
///
/// Measured on TypeScript 7.0.2: `(typeof KP)['st']` and `(typeof
/// KP2)['st']` are `"static"`, `ConstructorParameters<typeof KP>[0]` is
/// `string` and `[1]` is `number`, as is `ConstructorParameters<typeof
/// KP2>[1]`; `ConstructorParameters<typeof KO>[0]` is `number` (the last of
/// the two inherited overloads); `ConstructorParameters<typeof KG>[0]` is
/// `string`; `ConstructorParameters<typeof KE>` and
/// `ConstructorParameters<typeof KD>` are `[]` (`[] extends …` and `… extends
/// []` are `"y"`).
#[test]
fn a_value_base_gives_the_static_side_its_members_and_construct_signatures() {
    let failures = mismatches(
        VALUE_BASES,
        &[
            ("(typeof KP)['st']", "\"static\""),
            ("(typeof KP2)['st']", "\"static\""),
            ("ConstructorParameters<typeof KP>[0]", "string"),
            ("ConstructorParameters<typeof KP>[1]", "number"),
            ("ConstructorParameters<typeof KP2>[1]", "number"),
            ("ConstructorParameters<typeof KO>[0]", "number"),
            ("ConstructorParameters<typeof KG>[0]", "string"),
            (
                "[] extends ConstructorParameters<typeof KE> ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "ConstructorParameters<typeof KE> extends [] ? 'y' : 'n'",
                "\"y\"",
            ),
            (
                "ConstructorParameters<typeof KD> extends [] ? 'y' : 'n'",
                "\"y\"",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union of constructors gives no base type — its union signature still
/// constructs — a value without a construct signature gives none, and a
/// construct signature whose result is not an object type (a primitive, a
/// union) gives none either (TS2509).
///
/// Measured on TypeScript 7.0.2: `KU extends { a: 1 }`, `KU3 extends { a:
/// 1 }`, `KNC extends { x: 1 }`, `KS extends string` and `KUU extends {
/// a: 1 }` are `"n"`, `{} extends KU` and `{} extends KNC` are `"y"`,
/// `ConstructorParameters<typeof KU3>[0]` is `never`, and `{ own: 1 }
/// extends KS` and `{} extends KUU` are `"y"`.
#[test]
fn a_union_or_non_constructor_value_gives_no_base_type() {
    let failures = mismatches(
        VALUE_BASES,
        &[
            ("KU extends { a: 1 } ? 'y' : 'n'", "\"n\""),
            ("{} extends KU ? 'y' : 'n'", "\"y\""),
            ("KU3 extends { a: 1 } ? 'y' : 'n'", "\"n\""),
            ("ConstructorParameters<typeof KU3>[0]", "never"),
            ("KNC extends { x: 1 } ? 'y' : 'n'", "\"n\""),
            ("{} extends KNC ? 'y' : 'n'", "\"y\""),
            ("KS extends string ? 'y' : 'n'", "\"n\""),
            ("{ own: 1 } extends KS ? 'y' : 'n'", "\"y\""),
            ("KUU extends { a: 1 } ? 'y' : 'n'", "\"n\""),
            ("{} extends KUU ? 'y' : 'n'", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
