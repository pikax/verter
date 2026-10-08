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
/// function returns: each local class declares its own members, a local
/// class extending another derives from it (its `extends` name resolves
/// to the local class, not to anything of the file's), one extending a
/// file-scope class derives from that class, and a class with the same
/// shape that extends nothing derives from no other class.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`):
/// `Ext<R[0], R[1]>` (`LS` to `LB`),
/// `Ext<R[3], PR>` (`LP extends PR`) and `Ext<R[4], R[1]>` (a class
/// expression extending `LB`) are `"y"`; `Ext<R[1], R[0]>`, `Ext<R[2],
/// R[1]>` (`LO`, redeclaring `LB`'s shape), `Ext<PR, R[3]>` and `Ext<R[1],
/// R[4]>` are `"n"`, with `R` the tuple `ReturnType<typeof locals>`.
#[test]
fn a_function_local_class_declares_its_own_members() {
    let failures = mismatches(
        FIXTURE,
        &[
            (
                "Ext<ReturnType<typeof locals>[0], ReturnType<typeof locals>[1]>",
                "\"y\"",
            ),
            (
                "Ext<ReturnType<typeof locals>[1], ReturnType<typeof locals>[0]>",
                "\"n\"",
            ),
            (
                "Ext<ReturnType<typeof locals>[2], ReturnType<typeof locals>[1]>",
                "\"n\"",
            ),
            ("Ext<ReturnType<typeof locals>[3], PR>", "\"y\""),
            ("Ext<PR, ReturnType<typeof locals>[3]>", "\"n\""),
            (
                "Ext<ReturnType<typeof locals>[4], ReturnType<typeof locals>[1]>",
                "\"y\"",
            ),
            (
                "Ext<ReturnType<typeof locals>[1], ReturnType<typeof locals>[4]>",
                "\"n\"",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The same probes read through a type alias of the tuple (`type L =
/// ReturnType<typeof locals>`), which publishes the function's return
/// through the flow proof. TypeScript 7.0.2 answers them as above.
///
/// The flow proof reads a local class declaration's capture set as a typed
/// gap, so the alias publishes the tuple unverified and every probe stays a
/// partial demand until the proof enumerates that set.
#[test]
#[ignore = "the flow proof does not enumerate a local class declaration's capture set"]
fn a_function_local_class_read_through_an_alias_declares_its_own_members() {
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

/// A class expression's instance relates through its surface in every
/// position, an array or tuple element included.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`):
/// `Ext<[I<typeof mk>], [{}]>` and
/// `Ext<I<typeof mk>[], {}[]>` are `"y"`.
#[test]
fn a_class_expression_instance_relates_as_an_element() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("Ext<[I<typeof mk>], [{}]>", "\"y\""),
            ("Ext<I<typeof mk>[], {}[]>", "\"y\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const HERITAGE: &str = "\
class PR { protected x = 1 }
type Ext<A, B> = [A] extends [B] ? 'y' : 'n';
type Ctor<T = {}> = new (...args: any[]) => T;
function Tagged<B extends Ctor>(base: B) { return class extends base { protected tag = 't' as const }; }
class TB { protected z = 1 }
const CE1 = class { protected x = 1 };
const CE2 = class extends CE1 { protected x = 2 };
const CE3 = class extends CE2 { protected x = 3 };
function loc() { class L extends Tagged(TB) { protected z = 2 } return new L(); }
class G extends Tagged(TB) { protected z = 3 }
";

/// A class expression extending a class expression derives from it, and
/// from its base in turn: the `extends` name resolves through the class
/// index to the class expression the `const` holds.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`):
/// `Ext<InstanceType<typeof CE2>, InstanceType<typeof CE1>>` and
/// `Ext<InstanceType<typeof CE3>, InstanceType<typeof CE1>>` are `"y"`;
/// `Ext<InstanceType<typeof CE1>, InstanceType<typeof CE2>>` is `"n"`.
#[test]
fn a_class_expression_extending_a_class_expression_derives_from_it() {
    let failures = mismatches(
        HERITAGE,
        &[
            (
                "Ext<InstanceType<typeof CE2>, InstanceType<typeof CE1>>",
                "\"y\"",
            ),
            (
                "Ext<InstanceType<typeof CE3>, InstanceType<typeof CE1>>",
                "\"y\"",
            ),
            (
                "Ext<InstanceType<typeof CE1>, InstanceType<typeof CE2>>",
                "\"n\"",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class expression extending a class expression relates to an unrelated
/// class with a protected member of the same name as the checker does.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`):
/// `Ext<InstanceType<typeof CE2>, PR>` is `"n"`. The instance a `const`
/// holding `class extends CE1 { … }` constructs composes the instance of the
/// base VALUE `CE1`, which names no type, beneath its own members.
#[test]
fn a_class_expression_extending_a_class_expression_is_not_an_unrelated_class() {
    let failures = mismatches(HERITAGE, &[("Ext<InstanceType<typeof CE2>, PR>", "\"n\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class extending a mixin application derives from the mixin's base.
/// Relating `G` reaches the generic call `Tagged(TB)` in its heritage
/// clause, whose candidate session opens beneath the relation frames, so
/// the relation closes as its own SCC root around that call.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`):
/// `Ext<G, TB>` over `class G extends Tagged(TB) { protected z = 3 }` is
/// `"y"` and `Ext<TB, G>` `"n"`.
#[test]
fn a_class_extending_a_mixin_application_derives_from_its_base() {
    let failures = mismatches(
        HERITAGE,
        &[("Ext<G, TB>", "\"y\""), ("Ext<TB, G>", "\"n\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A function-local class extending a mixin application derives from the
/// mixin's base, as a file-scope class does.
///
/// Measured on TypeScript 7.0.2 (all four settings, `--noEmit`): over a
/// function-local `class L extends Tagged(TB) { protected z = 2 }`
/// returned as an instance, `Ext<ReturnType<typeof loc>, TB>` is `"y"` and
/// `Ext<TB, ReturnType<typeof loc>>` `"n"`.
#[test]
fn a_function_local_class_extending_a_mixin_application_derives_from_its_base() {
    let failures = mismatches(
        HERITAGE,
        &[
            ("Ext<ReturnType<typeof loc>, TB>", "\"y\""),
            ("Ext<TB, ReturnType<typeof loc>>", "\"n\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A local class read inside its own lowering — a static initializer
/// constructing it, a circular `extends` — is an unmodelled position of
/// that lowering, never a lowering that re-enters itself: each function
/// still produces its value, the self-read degrading in place.
#[test]
fn a_local_class_read_inside_its_own_lowering_is_unmodelled() {
    use super::checker_probe_lane_tests::{is_unmodelled_in, ProbeProject};
    let source = "\
function selfStatic() { class A { static s = new A(); } return A; }
function selfExtends() { class L extends L {} return L; }
function mutual() { class P extends Q {} class Q extends P {} return P; }
";
    for function in ["selfStatic", "selfExtends", "mutual"] {
        assert!(
            is_unmodelled_in(ProbeProject::default(), source, function),
            "{function}"
        );
    }
}

/// A member read through `this` selects only the class elements that
/// declare it: the class's other members never lower. The work count is
/// the number of class elements a demand lowered, so it depends on the
/// elements declaring the demanded name alone — not on how many other
/// members the class has.
#[test]
fn a_member_read_through_this_lowers_only_the_elements_declaring_it() {
    use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host, ProbeProject};
    let lowered_by = |source: &str, probe: &str| {
        let host = default_probe_host();
        host.provenance().reset();
        with_probe_on_host(&host, ProbeProject::default(), source, probe, |_, _| ());
        host.provenance().snapshot().class_elements_lowered
    };
    let unrelated: String = (0..40).map(|i| format!("  u{i} = {i};\n")).collect();
    let field = |extra: &str| format!("class W {{\n  a: string = 's';\n  b = this.a;\n{extra}}}\n");
    let narrow = lowered_by(&field(""), "W['b']");
    let wide = lowered_by(&field(&unrelated), "W['b']");
    assert!(narrow > 0, "the read lowered the element declaring `a`");
    assert!(
        narrow <= 2,
        "only the one element declaring `a` lowers; got {narrow}"
    );
    assert_eq!(narrow, wide, "40 unrelated members add no lowering");

    let overloaded = |extra: &str| {
        format!(
            "class O {{\n  m(x: string): string;\n  m(x: number): number;\n  m(x: any) {{ return x; }}\n  n = this.m;\n{extra}}}\n"
        )
    };
    let narrow = lowered_by(&overloaded(""), "O['n']");
    let wide = lowered_by(&overloaded(&unrelated), "O['n']");
    assert!(
        narrow >= 3,
        "the overloads and the implementation of `m` all lower; got {narrow}"
    );
    assert_eq!(narrow, wide, "40 unrelated members add no lowering");
}

/// A class expression extending a value with several construct signatures
/// inherits the FIRST signature's instance, as the class declaration of the
/// same base does.
///
/// Measured on TypeScript 7.0.2 (`--noEmit`): over `KO`,
/// `InstanceType<typeof C>['o']` is `"str"` for the class expression and for
/// `class D extends KO`.
#[test]
fn a_class_expression_extending_overloaded_constructors_inherits_the_first_signature() {
    let source = "\
declare const KO: { new (a: string): { o: 'str' }; new (a: number): { o: 'num' } };
const C = class extends KO { p = 1 };
class D extends KO { p = 1 }
";
    let failures = mismatches(
        source,
        &[
            ("InstanceType<typeof C>['o']", "\"str\""),
            ("InstanceType<typeof D>['o']", "\"str\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class a namespace holds reads its own members through `this`: the
/// selective member route serves a namespace member class as it does a
/// file-scope one.
#[test]
fn a_namespaced_class_reads_its_own_member_through_this() {
    let source = "\
namespace N { export class C { x = 'x' as const; read() { return this.x } y = this.x } }
";
    let failures = mismatches(
        source,
        &[("N.C['y']", "\"x\""), ("ReturnType<N.C['read']>", "\"x\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
