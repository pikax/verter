//! Differential probes of classes: instance and static members,
//! accessors, parameter properties, `this` types, heritage and abstract
//! classes, generic bases, declaration merging, mixins, class expressions
//! and class values. A row is a type in TYPE position over the fixture, or
//! a function of the fixture answered as its body-derived return.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on the exact
//! fixture with `tsc --ignoreConfig --declaration --emitDeclarationOnly
//! --strict --noErrorTruncation` under each `strictNullChecks` ×
//! `noImplicitAny` setting: `declare const p: <probe>; const s: never =
//! p;` (a function row reads `ReturnType<typeof f>`) read off the TS2322
//! message. A row with one answer answers alike in the four settings; a row
//! with two answers gives the `strictNullChecks` answer then the answer
//! with it off. An ignored test asserts the measured answer for rows the
//! lane does not yet answer as the checker does; "wrong-but-clean" marks a
//! lane answer published complete and undegraded.

use super::differential_harness_tests::{Matrix, Read};

/// A class with fields, parameter properties, methods, accessors and statics.
const MEMBERS: &str = r##"
class Pt {
  x = 0;
  readonly y: number = 1;
  static origin = new Pt("o", 0);
  static count = 0;
  private secret = "s";
  protected prot = true;
  opt?: string;
  constructor(public tag: string, private hidden: number) {}
  move(dx: number) { this.x += dx; return this; }
  get len() { return this.x; }
  set len(v: number) { this.x = v; }
  static make() { return new Pt("t", 1); }
  static create(this: void) { return 1 as const; }
  clone(): Pt { return new Pt(this.tag, 0); }
  self() { return this; }
}
class OnlyGet { get g() { return "g" as const; } }
class Init { a = 1; b = this.a + 1; c = [this.a]; }
class Accessors { #v = 0; get v() { return this.#v; } set v(n: number) { this.#v = n; } }
"##;

/// Instance fields, `readonly` and optional fields, parameter properties,
/// accessors (a getter alone too), methods returning `this`, statics,
/// `InstanceType`, `ConstructorParameters`, `Parameters` and `keyof` over a
/// class read their declared or initializer-inferred types.
#[test]
fn class_members_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(MEMBERS);
    let mut failures = matrix.types(&[
        ("Pt['x']", "number"),
        ("Pt['y']", "number"),
        ("Pt['tag']", "string"),
        ("ReturnType<Pt['move']>", "Pt"),
        ("Pt['len']", "number"),
        ("ReturnType<Pt['self']>", "Pt"),
        ("(typeof Pt)['count']", "number"),
        ("ReturnType<typeof Pt.make>", "Pt"),
        ("ReturnType<typeof Pt.create>", "1"),
        ("InstanceType<typeof Pt>", "Pt"),
        (
            "ConstructorParameters<typeof Pt>",
            "[tag: string, hidden: number]",
        ),
        ("keyof Pt", "keyof Pt"),
        ("OnlyGet['g']", "\"g\""),
        ("keyof Accessors", "\"v\""),
        (
            "[Pt] extends [{ x: number; readonly y: number }] ? 1 : 2",
            "1",
        ),
        ("Parameters<Pt['move']>", "[dx: number]"),
    ]);
    failures.extend(matrix.nullness(&[(Read::Type("Pt['opt']"), "string | undefined", "string")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `clone(): Pt` inside `class Pt` returns `Pt`: the annotation names the
/// enclosing class.
///
/// What the lane gives:
/// - `ReturnType<Pt['clone']>`: the checker answers `Pt`; the lane measured
///   `<opaque RecursiveRef { name: "Pt", args: [] }>`.
#[test]
#[ignore = "a method whose declared return names its own class reads that class"]
fn a_method_declared_to_return_its_class_reads_the_class() {
    let matrix = Matrix::new(MEMBERS);
    let failures = matrix.types(&[("ReturnType<Pt['clone']>", "Pt")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `static origin = new Pt("o", 0)` is `Pt`: a static constructing its own
/// class, whose static `make` constructs it too.
///
/// What the lane gives:
/// - `(typeof Pt)['origin']`: the checker answers `Pt`; the lane measured
///   `Pt` degraded by UnresolvedValue (the construction re-enters the class's
///   static side).
#[test]
#[ignore = "a static property constructing its own class reads its instance type"]
fn a_static_constructing_its_own_class_is_its_instance_type() {
    let matrix = Matrix::new(MEMBERS);
    let failures = matrix.types(&[("(typeof Pt)['origin']", "Pt")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class fields whose initializers read `this`: the instance's fields
/// (declared before or after, annotated or inferred), itself, a method, an
/// accessor and a private field, a static reading the class, a cycle, and
/// calls over `this`.
const THIS_READS: &str = r##"
declare function id<T>(x: T): T;
class Init { a = 1; b = this.a + 1; c = [this.a]; }
class Use { b = this.a + 1; a = 1; }
class Self { s = this; t = this.m(); m() { return "x"; } }
class Circ { p = this.q; q = this.p; }
class Ann { a: string = "s"; b = this.a; }
class St { static a = 1; static b = this.a; }
class Acc { get g() { return 1; } h = this.g; }
class Priv { #p = 1; q = this.#p; }
class T2 { a = 1; b = id(this.a); c = id(this); static s = 1; static t = id(this.s); }
"##;

/// The checker's answers for [`THIS_READS`]: an instance field initializer
/// reads `this` as the class instance — a field declared after it too
/// (TS2729, still typed) — and a static one reads the class; two fields
/// reading each other are `any` (TS7022 under `noImplicitAny`).
///
/// Measured on TypeScript 7.0.2 (`--target es2022`), alike under every
/// setting.
const THIS_READ_ROWS: &[(&str, &str)] = &[
    ("Init['b']", "number"),
    ("Init['c']", "number[]"),
    ("Use['b']", "number"),
    ("Self['t']", "string"),
    ("Circ['p']", "any"),
    ("Ann['b']", "string"),
    ("(typeof St)['b']", "number"),
    ("Acc['h']", "number"),
    ("Priv['q']", "number"),
    ("T2['b']", "number"),
    ("T2['c']", "T2"),
    ("(typeof T2)['t']", "number"),
];

/// A class field whose initializer reads `this` takes the type the checker
/// gives it ([`THIS_READ_ROWS`]).
///
/// What the lane gives: each row reduced to a partial demand, or a miss
/// degraded by UnresolvedValue for a static — never a published answer. A
/// declared class's field initializer lowers without a receiver, so the
/// field is outside the indexed expression domain.
#[test]
#[ignore = "a field initializer reading this reads the class instance or constructor type"]
fn a_field_initializer_reads_this() {
    let matrix = Matrix::new(THIS_READS);
    let failures = matrix.types(THIS_READ_ROWS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// No field whose initializer reads `this` publishes a complete,
/// undegraded answer the checker does not give: until the lane reads the
/// receiver, each is a gap.
///
/// Mutation: lowering such an initializer syntactically, as any other
/// field's, publishes `Init['b']` as a clean `any` and `T2['b']` as a
/// clean `unknown`.
#[test]
fn a_field_initializer_reading_this_is_never_a_clean_wrong_answer() {
    let matrix = Matrix::new(THIS_READS);
    let rows: Vec<(Read<'_>, Vec<&str>)> = THIS_READ_ROWS
        .iter()
        .map(|(probe, answer)| (Read::Type(probe), vec![*answer; 4]))
        .collect();
    let wrong_clean: Vec<String> = rows
        .iter()
        .zip(matrix.verdicts(&rows))
        .flat_map(|((read, _), verdicts)| {
            verdicts
                .into_iter()
                .filter(|verdict| !verdict.matched && verdict.class == "WRONG-CLEAN")
                .map(move |verdict| format!("{}: {}", read_text(read), verdict.lane))
        })
        .collect();
    assert!(wrong_clean.is_empty(), "{}", wrong_clean.join("\n"));
}

fn read_text<'a>(read: &Read<'a>) -> &'a str {
    match read {
        Read::Type(text) | Read::Return(text) => text,
    }
}

/// `get v() { return this.#v; }` beside `#v = 0` is `number`.
///
/// What the lane gives:
/// - `Accessors['v']`: the checker answers `number`; the lane reduced to a
///   partial demand.
#[test]
#[ignore = "an accessor pair over a #private field reads its getter's type"]
fn an_accessor_over_a_private_name_reads_its_getter() {
    let matrix = Matrix::new(MEMBERS);
    let failures = matrix.types(&[("Accessors['v']", "number")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class chains, `implements`, abstract members, generic bases, merged
/// interfaces and overloaded constructors.
const HERITAGE: &str = r##"
class Base { b = 1; m(): string { return "base"; } static sb = "sb"; }
class Mid extends Base { mid = 2; override m() { return "mid" as const; } }
class Leaf extends Mid { leaf = 3; }
interface Shape { area(): number }
class Sq implements Shape { area() { return 4; } side = 2; }
abstract class AbsB { abstract get n(): number; abstract run(): string; concrete() { return 1; } }
class ConcB extends AbsB { get n() { return 5; } run() { return "r"; } }
class GenBase<T> { v!: T; get(): T { return this.v; } }
class GenSub extends GenBase<string> {}
class GenSub2<U> extends GenBase<U[]> {}
class Chain { next(): this { return this; } }
class ChainSub extends Chain { sub = 1; }
interface Merged { extra: string }
class Merged { own = 1; }
class WithCtor { constructor(a: string); constructor(a: number); constructor(a: any) {} }
class OneCtor { constructor(a: string) {} }
class StaticInherit extends Base { static own = 2; }
"##;

/// A subclass reads its bases' members (overridden ones from itself), its
/// constructor type inherits statics, `implements` does not change the class
/// type, abstract members are implemented, a generic base is instantiated by
/// the `extends` clause, a merged interface adds members, and subclass
/// relations hold one way.
#[test]
fn class_heritage_reads_as_the_checker_reads_it() {
    let matrix = Matrix::new(HERITAGE);
    let failures = matrix.types(&[
        ("Leaf['b']", "number"),
        ("Leaf['mid']", "number"),
        ("ReturnType<Leaf['m']>", "\"mid\""),
        ("ReturnType<Base['m']>", "string"),
        ("(typeof Leaf)['sb']", "string"),
        ("(typeof StaticInherit)['own']", "number"),
        ("(typeof StaticInherit)['sb']", "string"),
        ("ReturnType<Sq['area']>", "number"),
        ("[Sq] extends [Shape] ? 1 : 2", "1"),
        ("ConcB['n']", "number"),
        ("ReturnType<ConcB['concrete']>", "number"),
        ("[typeof AbsB] extends [new () => AbsB] ? 1 : 2", "2"),
        ("GenSub['v']", "string"),
        ("ReturnType<GenSub['get']>", "string"),
        ("GenSub2<number>['v']", "number[]"),
        ("Merged['extra']", "string"),
        ("Merged['own']", "number"),
        ("InstanceType<typeof Leaf>", "Leaf"),
        ("[Leaf] extends [Base] ? 1 : 2", "1"),
        ("[Base] extends [Leaf] ? 1 : 2", "2"),
        ("keyof Leaf", "keyof Leaf"),
        ("[typeof Leaf] extends [typeof Base] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `next(): this` read as `ChainSub['next']` returns `ChainSub`: the
/// polymorphic `this` is instantiated with the receiver type.
///
/// What the lane gives:
/// - `ReturnType<ChainSub['next']>`: the checker answers `ChainSub`; the lane
///   measured `<unrendered BareRef(BareRefCarrier { name: "this", scope: File {
///   canonic>`.
#[test]
#[ignore = "a method declared to return this, read through a subclass, returns the subclass"]
fn a_polymorphic_this_return_reads_the_receiver_subclass() {
    let matrix = Matrix::new(HERITAGE);
    let failures = matrix.types(&[("ReturnType<ChainSub['next']>", "ChainSub")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// With `constructor(a: string); constructor(a: number); constructor(a: any)
/// {}`, the class's construct signatures are its overloads, never the
/// implementation: `ConstructorParameters<typeof WithCtor>` infers from the
/// LAST overload, `[a: number]`, and `typeof WithCtor` does not take a
/// `boolean` (TypeScript 7.0.2, all four settings alike).
#[test]
fn constructor_overloads_are_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(HERITAGE);
    let failures = matrix.types(&[
        ("ConstructorParameters<typeof WithCtor>", "[a: number]"),
        ("InstanceType<typeof WithCtor>", "WithCtor"),
        (
            "[typeof WithCtor] extends [new (a: string) => WithCtor] ? 1 : 2",
            "1",
        ),
        (
            "[typeof WithCtor] extends [new (a: boolean) => WithCtor] ? 1 : 2",
            "2",
        ),
        ("ConstructorParameters<typeof OneCtor>", "[a: string]"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Mixin functions returning class expressions, and class expressions held by
/// variables.
const MIXINS: &str = r##"
type Ctor<T = {}> = new (...args: any[]) => T;
function Tagged<TBase extends Ctor>(B: TBase) { return class extends B { tag = "t"; }; }
function Timed<TBase extends Ctor>(B: TBase) { return class extends B { time = 0; stamp() { return this.time; } }; }
class Plain { p = 1; }
const TaggedPlain = Tagged(Plain);
const Both = Timed(Tagged(Plain));
class UsesMixin extends Tagged(Plain) { own = true; }
const Expr = class { e = "e"; };
const NamedExpr = class Inner { n = 2; };
let Reassignable = class { r = 3; };
"##;

/// A single mixin application's instance carries the mixin's members and its
/// base's, a class extending a mixin call reads both, and a class expression
/// held by a `const` or `let` (named or not) reads its members.
#[test]
fn class_expressions_and_single_mixins_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(MIXINS);
    let failures = matrix.types(&[
        ("InstanceType<typeof TaggedPlain>['tag']", "string"),
        ("InstanceType<typeof TaggedPlain>['p']", "number"),
        ("UsesMixin['own']", "boolean"),
        ("UsesMixin['tag']", "string"),
        ("UsesMixin['p']", "number"),
        ("InstanceType<typeof Expr>['e']", "string"),
        ("InstanceType<typeof NamedExpr>['n']", "number"),
        ("InstanceType<typeof Reassignable>['r']", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `Timed(Tagged(Plain))` is an instance with `time`, `stamp`, `tag` and `p`,
/// and is below `Plain`.
///
/// What the lane gives:
/// - `InstanceType<typeof Both>['time']`: the checker answers `number`; the
///   lane measured `<opaque Miss>`.
/// - `InstanceType<typeof Both>['tag']`: the checker answers `string`; the lane
///   measured `<opaque Miss>`.
/// - `ReturnType<InstanceType<typeof Both>['stamp']>`: the checker answers
///   `number`; the lane measured `<opaque Miss>`.
/// - `[InstanceType<typeof Both>] extends [Plain] ? 1 : 2`: the checker answers
///   `1`; the lane measured `<unreduced conditional>`.
#[test]
#[ignore = "a mixin applied to a mixin application carries every layer's members"]
fn a_nested_mixin_application_carries_every_layer() {
    let matrix = Matrix::new(MIXINS);
    let failures = matrix.types(&[
        ("InstanceType<typeof Both>['time']", "number"),
        ("InstanceType<typeof Both>['tag']", "string"),
        ("ReturnType<InstanceType<typeof Both>['stamp']>", "number"),
        ("[InstanceType<typeof Both>] extends [Plain] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class values constructed, read, and returned.
const CLASS_VALUES: &str = r##"
class K { v = 1; static s = "s"; m() { return this.v; } }
class G<T> { constructor(public item: T) {} }
class Sub extends K { w = "w"; }
export function mkK() { return new K(); }
export function mkG() { return new G("x"); }
export function mkGNum() { return new G<number>(1); }
export function readV() { return new K().v; }
export function callM() { return new K().m(); }
export function staticRead() { return K.s; }
export function ctorValue() { return K; }
export function subOrBase(c: boolean) { return c ? new K() : new Sub(); }
export function thisInArrow() { return new (class { a = 1; f = () => this.a; })().f(); }
export function protoRead() { return K.prototype; }
export function ctorCall() { return new (ctorValue())().m(); }
"##;

/// `new` over a class or generic class (inferred or explicit) is its instance
/// type, member and method reads through it take their types, a static reads
/// through the class value, a conditional of a base and subclass instance
/// reduces to the base, and `prototype` is the instance type.
#[test]
fn class_values_read_as_the_checker_reads_them() {
    let matrix = Matrix::new(CLASS_VALUES);
    let failures = matrix.returns(&[
        ("mkK", "K"),
        ("mkG", "G<string>"),
        ("mkGNum", "G<number>"),
        ("readV", "number"),
        ("callM", "number"),
        ("staticRead", "string"),
        ("subOrBase", "K"),
        ("thisInArrow", "number"),
        ("protoRead", "K"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `return K;` is `typeof K`, the class's constructor type. The checker prints
/// the value by its name, `typeof K`; the lane prints the constructor type it
/// is, `{ new (): K; prototype: K; s: string; }` — a difference in print only:
/// every read of the value agrees, as each row here checks.
#[test]
fn a_returned_class_value_reads_as_typeof_the_class() {
    let matrix = Matrix::new(CLASS_VALUES);
    let mut failures = matrix.types(&[
        ("ReturnType<typeof ctorValue>['s']", "string"),
        ("InstanceType<ReturnType<typeof ctorValue>>", "K"),
        (
            "keyof ReturnType<typeof ctorValue>",
            "\"prototype\" | \"s\"",
        ),
        ("ReturnType<typeof ctorValue>['prototype']", "K"),
        (
            "[ReturnType<typeof ctorValue>] extends [typeof K] ? ([typeof K] extends [ReturnType<typeof ctorValue>] ? 1 : 2) : 3",
            "1",
        ),
    ]);
    failures.extend(matrix.returns(&[("ctorCall", "number")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class fields initialized by calls and constructions.
const FIELD_CALLS: &str = r##"
class Pt { x = 0; constructor(public tag: string, private hidden: number) {} }
declare function mk(): number;
declare function lit<T>(x: T): T;
class R { v = mk(); static s = mk(); readonly r = mk(); w = lit(1); readonly wr = lit(1); static ws = lit("s"); }
class Simple { n = 1; }
class Q2 { inst = new Simple(); static p = new Pt("q", 1); }
class S4 { static make() { return new S4(); } }
class Q6 { inst = new S4(); }
"##;

/// A field whose initializer's type derives from a call takes the call's
/// result, instance and static alike: `v = mk()` is `number`, `inst = new
/// Simple()` is `Simple`, `static p = new Pt("q", 1)` is `Pt`; a mutable
/// field widens a fresh literal result (`w = lit(1)` is `number`) and a
/// `readonly` one keeps it (`1`). Measured on TypeScript 7.0.2 under all four
/// settings.
#[test]
fn a_field_initialized_by_a_call_takes_the_calls_result() {
    let matrix = Matrix::new(FIELD_CALLS);
    let failures = matrix.types(&[
        ("R['v']", "number"),
        ("(typeof R)['s']", "number"),
        ("R['r']", "number"),
        ("R['w']", "number"),
        ("R['wr']", "1"),
        ("(typeof R)['ws']", "string"),
        ("Q2['inst']", "Simple"),
        ("(typeof Q2)['p']", "Pt"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `inst = new S4()` is `S4` where `S4` declares `static make() { return new
/// S4(); }`: constructing a class whose static method constructs it reads
/// its instance type.
///
/// What the lane gives:
/// - `Q6['inst']`: the checker answers `S4`; the lane reduced to a partial
///   demand (the static method's flow return re-enters the construction).
#[test]
#[ignore = "a field constructing a class whose static method constructs it reads its instance type"]
fn a_field_constructing_a_self_constructing_class_reads_its_instance_type() {
    let matrix = Matrix::new(FIELD_CALLS);
    let failures = matrix.types(&[("Q6['inst']", "S4")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class expressions carrying ECMAScript private names: two structurally
/// identical classes from different expressions, each with a `#private`
/// field, method or accessor.
const PRIVATE_BRANDS: &str = r##"
function a() { return class { #x = 1 }; }
function b() { return class { #x = 1 }; }
function c() { return class { #m() { return 1; } }; }
function d() { return class { #m() { return 1; } }; }
function e() { return class { get #g() { return 1; } }; }
function f() { return class { get #g() { return 1; } }; }
function g() { return class { #x = 1; y = 2 }; }
type IA = InstanceType<ReturnType<typeof a>>;
type IB = InstanceType<ReturnType<typeof b>>;
type IG = InstanceType<ReturnType<typeof g>>;
"##;

/// A class expression's `#private` field, method or accessor brands its
/// instance type with its own declaration: two class expressions of the
/// same shape are unrelated, a class relates to itself, an object without
/// the brand is not assignable to it, and its public members read as
/// always.
///
/// Measured on TypeScript 7.0.2, alike under every setting.
///
/// Mutation: lowering no member for a private name answers `IA extends IB`,
/// the method and accessor rows, `{} extends IA`, `IG extends IA` and `{ y:
/// number } extends IG` `1`.
#[test]
fn a_class_expressions_private_names_brand_its_instance_type() {
    let matrix = Matrix::new(PRIVATE_BRANDS);
    let failures = matrix.types(&[
        ("IA extends IB ? 1 : 2", "2"),
        ("IA extends IA ? 1 : 2", "1"),
        (
            "InstanceType<ReturnType<typeof c>> extends InstanceType<ReturnType<typeof d>> ? 1 : 2",
            "2",
        ),
        (
            "InstanceType<ReturnType<typeof e>> extends InstanceType<ReturnType<typeof f>> ? 1 : 2",
            "2",
        ),
        ("{} extends IA ? 1 : 2", "2"),
        ("IA extends {} ? 1 : 2", "1"),
        ("IG extends IA ? 1 : 2", "2"),
        ("{ y: number } extends IG ? 1 : 2", "2"),
        ("IG['y']", "number"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Class expressions whose keys `keyof` reads: returned from functions (with
/// a private name, several public members beside `private` and `protected`
/// ones, and a base), and held by module constants.
const KEYED_CLASS_EXPRESSIONS: &str = r##"
function a() { return class { #x = 1 }; }
function g() { return class { #x = 1; y = 2 }; }
function h() { return class { y = 2; z = 3; private p = 1; protected q = 2 }; }
class B { b = 1 }
function k() { return class extends B { own = 1 }; }
const A1 = class { #x = 1 };
let C1 = class { #x = 1; y = 2 };
const Expr = class { e = "e"; f = 1 };
class K2 { a = 1; b = 2 }
type IA = InstanceType<ReturnType<typeof a>>;
type IG = InstanceType<ReturnType<typeof g>>;
type IH = InstanceType<ReturnType<typeof h>>;
type IK = InstanceType<ReturnType<typeof k>>;
"##;

/// `keyof` a class expression's instance type is its public keys (a
/// `#private`, `private` or `protected` member is no key, a base's public
/// member is one): none is `never`, one is that key, and two or more keep the
/// class as their origin, `keyof (Anonymous class)` — a key set a relation
/// reads as its keys.
///
/// Measured on TypeScript 7.0.2 (`--target es2022`), alike under every
/// setting: `keyof IA` is `never`, `keyof IG` `"y"`, `keyof IH` and `keyof
/// IK` `keyof (Anonymous class)`, `keyof InstanceType<typeof A1>` `never`,
/// `keyof InstanceType<typeof C1>` `"y"`; the key-set equality checks each
/// answer `1`.
///
/// Mutation: reading `keyof` over a class expression's instance as a miss
/// leaves `keyof IA`, `keyof IG`, `keyof IH` and `keyof IK` their
/// declarations' `keyof` carriers and the key-set checks unreduced.
#[test]
fn keyof_a_class_expressions_instance_type_is_its_public_keys() {
    let matrix = Matrix::new(KEYED_CLASS_EXPRESSIONS);
    let failures = matrix.types(&[
        ("keyof IA", "never"),
        ("keyof IG", "\"y\""),
        ("keyof IH", "keyof (Anonymous class)"),
        ("keyof IK", "keyof (Anonymous class)"),
        ("keyof InstanceType<typeof A1>", "never"),
        ("keyof InstanceType<typeof C1>", "\"y\""),
        (
            "[keyof IH] extends [\"y\" | \"z\"] ? [\"y\" | \"z\"] extends [keyof IH] ? 1 : 2 : 3",
            "1",
        ),
        (
            "[keyof IK] extends [\"own\" | \"b\"] ? [\"own\" | \"b\"] extends [keyof IK] ? 1 : 2 : 3",
            "1",
        ),
        (
            "[keyof InstanceType<typeof Expr>] extends [\"e\" | \"f\"] ? [\"e\" | \"f\"] extends [keyof InstanceType<typeof Expr>] ? 1 : 2 : 3",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `keyof` a class's instance read through `InstanceType` keeps the class as
/// its origin: `keyof InstanceType<typeof K2>` prints `keyof K2` and `keyof
/// InstanceType<typeof Expr>` prints `keyof Expr` (the key sets agree, as
/// [`keyof_a_class_expressions_instance_type_is_its_public_keys`] checks).
///
/// Measured on TypeScript 7.0.2, alike under every setting.
///
/// What the lane gives: the key unions `"b" | "a"` and `"f" | "e"` — a
/// `keyof` over a library application reads no origin through it.
#[test]
#[ignore = "keyof over InstanceType of a class keeps the class as the key union's origin"]
fn keyof_an_instance_type_application_keeps_the_class_origin() {
    let matrix = Matrix::new(KEYED_CLASS_EXPRESSIONS);
    let failures = matrix.types(&[
        ("keyof InstanceType<typeof K2>", "keyof K2"),
        ("keyof InstanceType<typeof Expr>", "keyof Expr"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A class whose members are all private has no key: `[keyof K1] extends
/// [never]` is `1` over `class K1 { #x = 1 }`, and so over `class K3 {}` and
/// `class K4 { private p = 1 }`; `[keyof K5] extends ["y"]` over `class K5 {
/// #x = 1; y = 2 }` is `1`.
///
/// Measured on TypeScript 7.0.2 (`--target es2022`), alike under every
/// setting.
///
/// Mutation: reading the `keyof` below `never` with the bottom rule before
/// its key set settles answers the three `never` rows `2`.
#[test]
fn a_keyof_of_only_private_members_relates_as_never() {
    let matrix = Matrix::new(
        "class K1 { #x = 1 }\nclass K3 {}\nclass K4 { private p = 1 }\nclass K5 { #x = 1; y = 2 }\n",
    );
    let failures = matrix.types(&[
        ("[keyof K1] extends [never] ? 1 : 2", "1"),
        ("[keyof K3] extends [never] ? 1 : 2", "1"),
        ("[keyof K4] extends [never] ? 1 : 2", "1"),
        ("[keyof K5] extends [\"y\"] ? 1 : 2", "1"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
