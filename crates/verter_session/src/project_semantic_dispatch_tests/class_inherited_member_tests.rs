//! An inherited class member is read where a class on the inheritance path
//! declares it: the read follows the demanded name up the `extends` chain one
//! class at a time, stops at the first class that declares the name (an
//! override shadows its bases), and never lowers a base's other members.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this exact
//! fixture with `tsc --ignoreConfig --noEmit --target es2022` under
//! `--strict` and under `--strict --strictNullChecks false --noImplicitAny
//! false` (the settings agree on every probe) as `const v: "q" = null! as
//! <probe>;`, read off the TS2322 message.

use super::checker_probe_lane_tests::{
    default_probe_host, mismatches, with_probe_on_host, ProbeProject,
};

const FIXTURE: &str = "\
class A0 { a: string = 'a0'; protected p = 'p0' as const; static s = 'S0' as const; m() { return 'm0' as const } self() { return this } get g() { return 'g0' as const } }
class A1 extends A0 { a = 'a1' as const }
class A2 extends A1 { }
class G<T> { v!: T; w!: T[] }
class GD extends G<string> { }
class GD2<U> extends G<U> { }
class Same { a = 'I' as const; static a = 'S' as const }
class SameD extends Same {}
class PD extends A0 { q = this.p }
class PB { private z = 1 as const; read() { return this.z } }
class PBD extends PB {}
class HB { #h = 2 as const; get h() { return this.#h } }
class HD extends HB {}
class OB { o = 'base' as string; }
class OD extends OB { declare o: 'own' }
function r1(x: A2) { return x.a }
function r2(x: A2) { return x.m() }
function r3(x: GD) { return x.v }
function r4(x: GD2<number>) { return x.w }
function r5(x: SameD) { return x.a }
function r6(x: PD) { return x.q }
function r7() { return SameD.a }
function r8(x: PBD) { return x.read() }
function r9(x: HD) { return x.h }
function r10(x: A2) { return x.self() }
function r11(x: A2) { return x.g }
function r12(x: OD) { return x.o }
function r13() { return A2.s }
";

/// An inherited read answers from the nearest class on the path declaring
/// the name, with the `extends` clause's type arguments applied and the
/// member's visibility, accessor kind and `this` kept: an override in the
/// middle of the chain stops the lookup (`r1`), an own redeclaration stops it
/// at the reading class (`r12`), an instance read never reads a same-name
/// static (`r5`) and a static read never reads an instance member (`r7`,
/// `r13`); protected (`r6`), private (`r8`) and `#private` (`r9`) members a
/// base reads through `this` keep their types; a base method returning
/// `this` returns the reading receiver (`r10`).
///
/// Measured on TypeScript 7.0.2: `r1` is `"a1"`, `r2` `"m0"`, `r3` `string`,
/// `r4` `number[]`, `r5` `"I"`, `r6` `"p0"`, `r7` `"S"`, `r8` `1`, `r9` `2`,
/// `r10` `A2`, `r11` `"g0"`, `r12` `"own"` and `r13` `"S0"`.
#[test]
fn an_inherited_member_reads_where_the_nearest_class_declares_it() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<typeof r1>", "\"a1\""),
            ("ReturnType<typeof r2>", "\"m0\""),
            ("ReturnType<typeof r3>", "string"),
            ("ReturnType<typeof r4>", "number[]"),
            ("ReturnType<typeof r5>", "\"I\""),
            ("ReturnType<typeof r6>", "\"p0\""),
            ("ReturnType<typeof r7>", "\"S\""),
            ("ReturnType<typeof r8>", "1"),
            ("ReturnType<typeof r9>", "2"),
            ("ReturnType<typeof r10>", "A2"),
            ("ReturnType<typeof r11>", "\"g0\""),
            ("ReturnType<typeof r12>", "\"own\""),
            ("ReturnType<typeof r13>", "\"S0\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The semantic nodes one probe interned on a fresh host: a demand-scoped
/// work count that grows with every base member a read lowers.
fn nodes_interned_by(source: &str, probe: &str) -> u64 {
    let host = default_probe_host();
    host.provenance().reset();
    with_probe_on_host(&host, ProbeProject::default(), source, probe, |_, _| ());
    host.provenance().snapshot().node_arena_pushes
}

/// A member read through a derived class's instance follows only the
/// demanded name up the `extends` chain: forty more members on the base, or
/// on a class between the reader and the declaring base, add no work.
#[test]
fn an_inherited_member_read_lowers_no_unrelated_base_member() {
    let unrelated: String = (0..40).map(|i| format!("  u{i} = {i};\n")).collect();
    let through_reference = |base_extra: &str, middle_extra: &str| {
        format!(
            "class B {{\n  a: string = 's';\n{base_extra}}}\n\
             class M extends B {{\n{middle_extra}}}\n\
             class D extends M {{ }}\n\
             function f(d: D) {{ return d.a; }}\n"
        )
    };
    let narrow = nodes_interned_by(&through_reference("", ""), "ReturnType<typeof f>");
    assert_eq!(
        narrow,
        nodes_interned_by(&through_reference(&unrelated, ""), "ReturnType<typeof f>"),
        "forty unrelated base members add no work"
    );
    assert_eq!(
        narrow,
        nodes_interned_by(&through_reference("", &unrelated), "ReturnType<typeof f>"),
        "forty unrelated members of a class between add no work"
    );

    let through_this = |base_extra: &str| {
        format!(
            "class B {{\n  a: string = 's';\n{base_extra}}}\n\
             class D extends B {{ b = this.a; }}\n\
             function f(d: D) {{ return d.b; }}\n"
        )
    };
    assert_eq!(
        nodes_interned_by(&through_this(""), "ReturnType<typeof f>"),
        nodes_interned_by(&through_this(&unrelated), "ReturnType<typeof f>"),
        "a read through `this` lowers no unrelated base member"
    );
}

/// A base another module augments declares what the augmentation adds: an
/// inherited read through it reads the merged declaration, never a member
/// further up the chain that the augmentation shadows.
///
/// Measured on TypeScript 7.0.2 (`--strict --module esnext
/// --moduleResolution bundler`): over `B extends A` with `A { x: string }`
/// and another module's `declare module './base' { interface B { x: 'aug' }
/// }`, `ReturnType<typeof f>` for `f(d: D) { return d.x }` with `class D
/// extends B` is `"aug"`.
#[test]
fn an_inherited_read_through_an_augmented_base_reads_the_augmentation() {
    let failures = super::checker_probe_lane_tests::mismatches_in(
        ProbeProject {
            files: &[
                (
                    "base.ts",
                    "export class A { x: string = ''; }\nexport class B extends A { }\n",
                ),
                (
                    "aug.ts",
                    "import './base';\ndeclare module './base' { interface B { x: 'aug' } }\nexport {};\n",
                ),
            ],
            ..ProbeProject::default()
        },
        "import { B } from './base';\nimport './aug';\nclass D extends B { }\nfunction f(d: D) { return d.x; }\n",
        &[("ReturnType<typeof f>", "\"aug\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An inherited read across modules is invalidated by an edit to the member
/// it reads and by a class between the reader and the base starting to
/// declare that member, and still answers after an edit to another member.
///
/// Measured on TypeScript 7.0.2 (`--strict --module esnext
/// --moduleResolution bundler`), over `A` in `base.ts`, `M extends A` in
/// `mid.ts` and `D extends M` reading `d.a`: `string` with `a: string`,
/// `number` once `a: number`, `7` once `M` declares `a: 7 = 7`, and still
/// `7` after `A`'s unrelated `u` changes.
#[test]
fn an_inherited_read_follows_edits_to_the_member_it_reads() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node;
    let host = default_probe_host();
    let write = |name: &str, source: &str| {
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            &format!("/wb/{name}"),
            source,
            crate::FileLanguage::script_ts(),
        );
    };
    let probe =
        "import { M } from './mid';\nclass D extends M { }\nfunction f(d: D) { return d.a; }\n";
    let answer = || {
        with_probe_on_host(
            &host,
            ProbeProject::default(),
            probe,
            "ReturnType<typeof f>",
            |dispatch, node| render_node(dispatch, node, 0),
        )
    };
    let base = |a: &str, u: &str| format!("export class A {{ a!: {a}; u = {u}; }}\n");
    let mid = |own: &str| {
        format!("import {{ A }} from './base';\nexport class M extends A {{ {own} }}\n")
    };
    write("base.ts", &base("string", "1"));
    write("mid.ts", &mid(""));
    assert_eq!(answer(), "string");
    write("base.ts", &base("number", "1"));
    assert_eq!(answer(), "number", "an edit to the member read invalidates");
    write("mid.ts", &mid("a: 7 = 7;"));
    assert_eq!(
        answer(),
        "7",
        "a class between that starts declaring the member stops the lookup"
    );
    write("base.ts", &base("number", "2"));
    assert_eq!(answer(), "7", "an unrelated base edit keeps the answer");
}

/// A deep `extends` chain answers the inherited read at its far end: the
/// selective read follows the demanded name through every class of the chain.
#[test]
fn a_deep_extends_chain_answers_the_inherited_read() {
    let depth = 250;
    let mut source = String::from("class C0 { a = 'deep' as const }\n");
    for i in 1..=depth {
        source.push_str(&format!("class C{i} extends C{} {{}}\n", i - 1));
    }
    source.push_str(&format!("function r(x: C{depth}) {{ return x.a }}\n"));
    let failures = mismatches(&source, &[("ReturnType<typeof r>", "\"deep\"")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
