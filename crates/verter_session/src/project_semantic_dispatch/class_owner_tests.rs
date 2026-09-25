//! A member's declaring class is the class node whose body declares it —
//! a file-scope class, a class expression a function returns or a
//! variable holds, a mixin's class, a class declared inside a body — and
//! the protected-member rule reads derivation from that class: through
//! the class-heritage ancestry authority for a file-scope class, and for
//! a class with no `extends` clause from the clause's absence.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit --strict` under each of
//! `strictNullChecks` and `noImplicitAny` on and off (the four settings
//! agree on every probe) as `const v: "q" = null! as <probe>;`, read off
//! the TS2322 message. Declaration emit refuses the fixture (TS4094: a
//! protected member of an exported anonymous class).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
class PR { protected x = 1 }
type Ext<A, B> = [A] extends [B] ? 'y' : 'n';
type Ctor<T = {}> = new (...args: any[]) => T;
function Tagged<B extends Ctor>(base: B) { return class extends base { protected tag = 't' as const }; }
class TB { protected tag = 'x' as const }
const T1 = Tagged(TB);
const T2 = Tagged(TB);
function mk() { return class { protected z = 1 }; }
function mk2() { return class extends (mk()) { protected z = 2 }; }
function mk3() { return class { protected z = 1 }; }
function mkp() { return class extends PR { protected x = 2 }; }
function mkq() { return class { constructor(protected z: number) {} } }
function mkr() { return class extends (mkq()) { constructor(protected z: number) { super(z); } } }
const CE = class extends PR { protected x = 4 };
const CO = class { protected x = 4 };
function locals() {
  class LB { protected x = 1 }
  class LS extends LB { protected x = 2 }
  class LO { protected x = 1 }
  class LP extends PR { protected x = 3 }
  const LE = class extends LB { protected x = 4 };
  return [new LS(), new LB(), new LO(), new LP(), new LE()] as const;
}
type L = ReturnType<typeof locals>;
type I<F extends (...a: any) => any> = InstanceType<ReturnType<F>>;
";

/// A class expression a function returns declares its own members: one
/// relates to another's protected member only when it derives from that
/// class.
///
/// Measured on TypeScript 7.0.2: `Ext<I<typeof mk2>, I<typeof mk>>` and
/// `Ext<I<typeof mkp>, PR>` are `"y"`; `Ext<I<typeof mk>, I<typeof
/// mk2>>`, `Ext<I<typeof mk3>, I<typeof mk>>`, `Ext<I<typeof mk>, I<typeof
/// mk3>>` and `Ext<PR, I<typeof mkp>>` are `"n"`. A constructor parameter
/// declares a property too: over `mkq` (`constructor(protected z: number)`)
/// and `mkr` (extending `mkq()` and redeclaring it), `Ext<I<typeof mkr>,
/// I<typeof mkq>>` is `"y"` and `Ext<I<typeof mkq>, I<typeof mk>>`, `Ext<I<typeof
/// mk>, I<typeof mkq>>` and `Ext<I<typeof mkq>, I<typeof mkr>>` `"n"`.
#[test]
fn a_returned_class_expression_declares_its_own_members() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Ext<I<typeof mk2>, I<typeof mk>>", "\"y\""),
            ("Ext<I<typeof mk>, I<typeof mk2>>", "\"n\""),
            ("Ext<I<typeof mk3>, I<typeof mk>>", "\"n\""),
            ("Ext<I<typeof mk>, I<typeof mk3>>", "\"n\""),
            ("Ext<PR, I<typeof mkp>>", "\"n\""),
            ("Ext<I<typeof mkp>, PR>", "\"y\""),
            ("Ext<I<typeof mkq>, I<typeof mk>>", "\"n\""),
            ("Ext<I<typeof mk>, I<typeof mkq>>", "\"n\""),
            ("Ext<I<typeof mkr>, I<typeof mkq>>", "\"y\""),
            ("Ext<I<typeof mkq>, I<typeof mkr>>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class expression a variable holds, and a mixin's class over the base
/// its instantiation supplies.
///
/// Measured on TypeScript 7.0.2: `Ext<InstanceType<typeof CE>, PR>`,
/// `Ext<InstanceType<typeof T1>, InstanceType<typeof T2>>` and
/// `Ext<InstanceType<typeof T1>, TB>` are `"y"`; `Ext<PR,
/// InstanceType<typeof CE>>`, `Ext<InstanceType<typeof CO>, PR>`, `Ext<PR,
/// InstanceType<typeof CO>>`, `Ext<InstanceType<typeof CO>,
/// InstanceType<typeof CE>>` and `Ext<TB, InstanceType<typeof T1>>` are
/// `"n"`.
#[test]
fn a_held_class_expression_and_a_mixin_class_declare_their_own_members() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Ext<InstanceType<typeof CE>, PR>", "\"y\""),
            ("Ext<PR, InstanceType<typeof CE>>", "\"n\""),
            ("Ext<InstanceType<typeof CO>, PR>", "\"n\""),
            ("Ext<PR, InstanceType<typeof CO>>", "\"n\""),
            (
                "Ext<InstanceType<typeof CO>, InstanceType<typeof CE>>",
                "\"n\"",
            ),
            (
                "Ext<InstanceType<typeof T1>, InstanceType<typeof T2>>",
                "\"y\"",
            ),
            ("Ext<InstanceType<typeof T1>, TB>", "\"y\""),
            ("Ext<TB, InstanceType<typeof T1>>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Classes declared inside a function body, read through the tuple the
/// function returns.
///
/// Measured on TypeScript 7.0.2: `Ext<L[0], L[1]>` (`LS` to `LB`),
/// `Ext<L[3], PR>` (`LP extends PR`) and `Ext<L[4], L[1]>` (a class
/// expression extending `LB`) are `"y"`; `Ext<L[1], L[0]>`, `Ext<L[2],
/// L[1]>` (`LO`, redeclaring `LB`'s shape), `Ext<PR, L[3]>` and `Ext<L[1],
/// L[4]>` are `"n"`.
///
/// A function-local class's instance is not evaluated here yet: every
/// probe reads an unmodelled position and stays undecided until local
/// classes evaluate.
#[test]
#[ignore = "function-local classes do not evaluate yet"]
fn a_function_local_class_declares_its_own_members() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Ext<L[0], L[1]>", "\"y\""),
            ("Ext<L[1], L[0]>", "\"n\""),
            ("Ext<L[2], L[1]>", "\"n\""),
            ("Ext<L[3], PR>", "\"y\""),
            ("Ext<PR, L[3]>", "\"n\""),
            ("Ext<L[4], L[1]>", "\"y\""),
            ("Ext<L[1], L[4]>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
