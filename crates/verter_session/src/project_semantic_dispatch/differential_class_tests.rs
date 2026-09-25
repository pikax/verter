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

/// `static origin = new Pt("o", 0)` is `Pt`. Wrong-but-clean: the lane answers
/// `any`.
///
/// What the lane gives:
/// - `(typeof Pt)['origin']`: the checker answers `Pt`; the lane measured
///   `any`.
#[test]
#[ignore = "a static property initialized with new C() is C"]
fn wrong_clean_a_static_initialized_with_new_is_its_instance_type() {
    let matrix = Matrix::new(MEMBERS);
    let failures = matrix.types(&[("(typeof Pt)['origin']", "Pt")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `b = this.a + 1` is `number` and `c = [this.a]` is `number[]`: a property
/// initializer reads `this` as the class instance. Wrong-but-clean: the lane
/// answers `any` / `any[]`.
///
/// What the lane gives:
/// - `Init['b']`: the checker answers `number`; the lane measured `any`.
/// - `Init['c']`: the checker answers `number[]`; the lane measured `any[]`.
#[test]
#[ignore = "a field initializer reading this.<field> takes that field's type"]
fn wrong_clean_a_field_initializer_reads_this() {
    let matrix = Matrix::new(MEMBERS);
    let failures = matrix.types(&[("Init['b']", "number"), ("Init['c']", "number[]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
/// {}`, `ConstructorParameters<typeof WithCtor>` infers from the LAST overload,
/// `[a: number]`; the implementation signature is not part of the type.
/// Wrong-but-clean: the lane answers `[a: any]`.
///
/// What the lane gives:
/// - `ConstructorParameters<typeof WithCtor>`: the checker answers `[a:
///   number]`; the lane measured `[a: any]`.
#[test]
#[ignore = "an overloaded constructor exposes its overloads, never its implementation signature"]
fn wrong_clean_constructor_parameters_read_the_last_overload() {
    let matrix = Matrix::new(HERITAGE);
    let failures = matrix.types(&[("ConstructorParameters<typeof WithCtor>", "[a: number]")]);
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

/// `return K;` is `typeof K`, the class's constructor type. Wrong-but-clean
/// (presentation): the lane expands it to the structural object `{ new (): K;
/// prototype: K; s: string; }`.
///
/// What the lane gives:
/// - `ctorValue`: the checker answers `typeof K`; the lane measured `{ new ():
///   K; prototype: K; s: string; }`.
#[test]
#[ignore = "a returned class value is typeof the class"]
fn wrong_clean_a_returned_class_value_prints_as_typeof_the_class() {
    let matrix = Matrix::new(CLASS_VALUES);
    let failures = matrix.returns(&[("ctorValue", "typeof K")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
