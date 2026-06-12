// Vendored fixture source bytes for oracle rows (`class_features` /
// `function_advanced` / `branded_types` / `decorators` /
// `substitution_types` / `typescript_rules` / `deep_path`),
// `include!`'d by `oracle_query_specs.rs` (the registry
// is the source-byte authority; the guard
// `inlined_registry_source_is_byte_identical_to_fixture_files` pins each const
// byte-identical to its on-disk `fixtures/*.ts` sibling). A separate file so
// the registry file holds the spec TABLE, not kilobytes of source payload.

/// Vendored source bytes of `/fixtures/class_features.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/class_features.ts`.
#[allow(dead_code)]
pub(crate) const CLASS_FEATURES_SOURCE: &str = r##"// @ai-generated - Synthetic class-features typeinfo fixture.
//
// Covers abstract classes, static inheritance, `extends` + `implements`,
// generic class hierarchies (with substituted type parameters), the
// `ConstructorParameters` utility over a subclass, deep `InstanceType` of
// a generic subclass, and a `protected` member referenced from a subclass
// method body.

// 1. Abstract class with abstract method + concrete subclass.
export abstract class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  abstract sound(): string;
}

export class Dog extends Animal {
  sound() {
    return "woof" as const;
  }
}

export type DogInstance = InstanceType<typeof Dog>;
export type DogSoundReturn = ReturnType<Dog["sound"]>;

// 2. Static inheritance: base owns a static field/method; subclass inherits it.
//
// `initial` is intentionally typed `string` (not `number = 0` — that would let
// an implementer pass via numeric-literal-widening of the initialiser rather
// than actually walking the static chain to the declared annotation).
export class BaseCounter {
  static initial: string = "0";
  static describe(): string {
    return "counter";
  }
}

export class StepCounter extends BaseCounter {}

export type StepCounterInitial = typeof StepCounter.initial;
export type StepCounterDescribeReturn = ReturnType<typeof StepCounter.describe>;

// 3. `extends` + `implements`. The instance must expose `name` (inherited)
// AND `greet` (implemented from the interface).
export class Named {
  name: string = "";
}

export interface Greetable {
  greet(): string;
}

export class Greeter extends Named implements Greetable {
  greet() {
    return "hello";
  }
}

export type GreeterInstance = InstanceType<typeof Greeter>;

// 4. Generic class hierarchy with substituted type parameter.
export class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

export class StringBox extends Box<string> {
  suffix() {
    return ".str" as const;
  }
}

export type StringBoxInstance = InstanceType<typeof StringBox>;

// 5. ConstructorParameters: subclass adds its own constructor that takes
// a different parameter list. ConstructorParameters resolves the subclass's
// ctor, NOT the parent's.
//
// The two subclass-ctor params are intentionally DIFFERENT primitives
// (`string` and `number`) so the assertion can distinguish "found subclass
// ctor" from "found base ctor + padded with another string" or "swapped
// argument order" — both would slip past a [string, string] check.
export class BaseShape {
  constructor(public id: string) {}
}

export class ColouredShape extends BaseShape {
  constructor(
    id: string,
    public count: number,
  ) {
    super(id);
  }
}

export type ColouredShapeCtorParams = ConstructorParameters<typeof ColouredShape>;

// 7. Protected member visible inside a subclass method body — TS infers the
// method return type from `this.value + "!"`, which is `string` (string
// concatenation of `protected value: string`).
//
// `value` is `string` (not `number`) on purpose. With a numeric `value` and
// `value + 1`, an implementer could pass the assertion via a generic
// "binary arithmetic → number" fallback without actually walking the base
// class chain to find `value`. With string-concat, the only way the result is
// `string` is to genuinely resolve `value` against the base class declaration.
export class ProtectedHolder {
  protected value: string = "";
}

export class ProtectedConsumer extends ProtectedHolder {
  bumped() {
    return this.value + "!";
  }
}

export type ProtectedConsumerBumpedReturn = ReturnType<ProtectedConsumer["bumped"]>;

// 8. Generic subclass with its OWN type parameter `U`, extending `Box<U>`.
// When instantiated as `Wrapper<string>`, the inherited `value: T` from
// `Box<T>` propagates `U = string` through, yielding `value: string`. The
// subclass adds `tag(): "wrapped"`.
//
// TS7 contract:
//   Wrapper<string> = { value: string; tag(): "wrapped" }
//   keyof Wrapper<string> = "value" | "tag"
//
// Note: `InstanceType<typeof Wrapper<string>>` is not the construct used
// here. `typeof Wrapper` is a generic constructor type; the canonical way
// to obtain the instance shape with a concrete type argument is the
// direct reference `Wrapper<string>`.
export class Wrapper<U> extends Box<U> {
  tag(): "wrapped" {
    return "wrapped";
  }
}

export type WrapperOfString = Wrapper<string>;

// 9. Static generic method instantiated via a type-argument expression on
// `typeof Class.method<...>` (instantiation expressions).
//
// `GenericStatic.make<T>(value: T): { wrapped: T }`. Instantiating
// `typeof GenericStatic.make<string>` yields `(value: string) => { wrapped: string }`.
// Its `ReturnType` is the object literal `{ wrapped: string }`.
//
// TS7 contract:
//   StaticMethodInstantiated = { wrapped: string }
//   keyof StaticMethodInstantiated = "wrapped"
export class GenericStatic {
  static make<T>(value: T): { wrapped: T } {
    return { wrapped: value };
  }
}

export type StaticMethodInstantiated = ReturnType<typeof GenericStatic.make<string>>;

// 10. `#private` field is NOT visible on the type-level instance projection.
// The `#secret` name is a brand that gates assignability, but `keyof`
// excludes it and `InstanceType<typeof PrivateHolder>` exposes only the
// public surface.
//
// TS7 contract:
//   InstanceType<typeof PrivateHolder> exposes only `visible: string`.
//   `"#secret"`/`"secret"` are NOT keys of the published instance.
export class PrivateHolder {
  #secret: number = 0;
  public visible: string = "";
}

export type PrivateHolderInstance = InstanceType<typeof PrivateHolder>;

// 11. TS `public` / `protected` / `private` keyword accessibility on a class.
// `keyof MixedVis` yields ONLY the public key `"a"` (TS excludes
// protected/private from the keyspace). A mapped type over the class
// (`Partial<MixedVis>`) carries ONLY the public member, and `Pick<MixedVis, 'a'>`
// materialises only `a`. These exercise the keyof/mapped/Pick keyspace gate.
//
// TS7 contract:
//   keyof MixedVis = "a"
//   Partial<MixedVis> = { a?: string }
//   Pick<MixedVis, "a"> = { a: string }
export class MixedVis {
  public a: string = "";
  protected b: number = 0;
  private c: boolean = false;
}

export type MixedVisKeyof = keyof MixedVis;
export type MixedVisPartial = Partial<MixedVis>;
export type MixedVisPick = Pick<MixedVis, "a">;
// `Record<keyof MixedVis, 1>` reifies the keyspace into an object surface so the
// produced keys are directly observable; only the public key `a` survives.
export type MixedVisRecord = Record<keyof MixedVis, 1>;

// DISCRIMINATING public-keyspace derivation fixtures (B4.5 derivation half).
// TS rejects each of these constraint/index positions because the named key is
// not a member of `keyof MixedVis` (public-only) — `b` is protected, `c` is
// private. The shared typed-IR derivation must therefore yield an EMPTY surface
// (Pick/Omit) or a miss (indexed access), never re-mint the non-public member.
// These fixtures are resolver inputs; they intentionally encode the TS-error
// positions to characterise the derivation, so the file is not expected to pass
// `tsc --noEmit`.
//
// `Pick<MixedVis, K>` over a NON-public key: empty surface (K ∉ keyof MixedVis).
// @ts-expect-error 'b' is protected and not in keyof MixedVis
export type MixedVisPickProtected = Pick<MixedVis, "b">;
// @ts-expect-error 'c' is private and not in keyof MixedVis
export type MixedVisPickPrivate = Pick<MixedVis, "c">;
// `Omit<MixedVis, "a">` = `Pick<MixedVis, Exclude<keyof MixedVis, "a">>`. Since
// `keyof MixedVis` is `"a"` only, the result is the EMPTY surface — the
// non-public `b` / `c` must NOT survive into the omitted surface.
export type MixedVisOmitPublic = Omit<MixedVis, "a">;
// Direct indexed access of a NON-public key: a miss (the value type of a
// private/protected member is not externally accessible via index).
// @ts-expect-error 'c' is private; external index access is not allowed
export type MixedVisIndexedPrivate = MixedVis["c"];
// Nested mapped-then-indexed access of a NON-public key over a class: the mapped
// surface (`Partial<MixedVis>`) carries only public keys, so indexing `["c"]`
// is a miss.
// @ts-expect-error 'c' is not a public key of Partial<MixedVis>
export type MixedVisPartialIndexedPrivate = Partial<MixedVis>["c"];

// 12. Ordinary (non-macro) UNION common-member surface. The TS member-access
// surface of `UnionA | UnionB` is the COMMON members only, and each common
// member's accessibility folds to the MOST RESTRICTIVE across the arms. `shared`
// is public in `UnionA` but private in `UnionB`, so the merged `shared` is
// private. The arm-only members (`onlyA` / `onlyB`) are not common and do not
// appear.
//
// TS7 contract: the common-member surface of `UnionA | UnionB` is `{ shared }`,
// and `shared` is private (so it does not reach any published surface).
export class UnionA {
  shared: string = "";
  onlyA: number = 0;
}

export class UnionB {
  private shared: string = "";
  onlyB: boolean = false;
}

export type UnionAB = UnionA | UnionB;
"##;

/// The workspace-file set the class_features rows upsert.
#[allow(dead_code)]
const CLASS_FEATURES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/class_features.ts",
    source: CLASS_FEATURES_SOURCE,
}];

/// Vendored source bytes of `/fixtures/function_advanced.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/function_advanced.ts`.
#[allow(dead_code)]
pub(crate) const FUNCTION_ADVANCED_SOURCE: &str = r#"// @ai-generated - Synthetic advanced-function typeinfo fixture.

// (1) Declaration-level overloads. TS7: at a call site, the matching overload
// signature is picked. `ReturnType<typeof lookup>` of an overloaded function
// returns the LAST signature's return type (the implementation signature is
// not part of the externally visible call signature list, so the last
// declared overload is what ReturnType sees).
export function lookup(key: "name"): string;
export function lookup(key: "count"): number;
export function lookup(key: "active"): boolean;
export function lookup(key: string): string | number | boolean {
  return null as any;
}

export function callLookupCount() {
  return lookup("count");
}
export type LookupCountResult = ReturnType<typeof callLookupCount>;
export type LookupReturnType = ReturnType<typeof lookup>;

// (2) `this` parameter typing — does not appear in `Parameters`.
export function withReceiver(this: { value: number }, factor: number): number {
  return this.value * factor;
}
export type WithReceiverParams = Parameters<typeof withReceiver>;
export type WithReceiverThis = ThisParameterType<typeof withReceiver>;
export type WithReceiverOmitThis = OmitThisParameter<typeof withReceiver>;

// (3) Constructor types.
export type Ctor = new (id: string) => { id: string; ready: boolean };
export type CtorParams = ConstructorParameters<Ctor>;
export type CtorInstance = InstanceType<Ctor>;

// (4) Generic function type alias.
export type Mapper<T, R> = (input: T) => R;
export type StringToNumberMapper = Mapper<string, number>;

// (5) Call + construct signatures. The interface is callable AS WELL AS
// constructable.
export interface Callable {
  (a: number): string;
  new (b: string): { value: number };
}
export declare const callable: Callable;
export type CallableCallParams = Parameters<typeof callable>;
export type CallableCallReturn = ReturnType<typeof callable>;
export type CallableCtorParams = ConstructorParameters<typeof callable>;
export type CallableCtorInstance = InstanceType<typeof callable>;

// (6) Higher-order function returning function. We instantiate concretely so
// the resolved return is a concrete `(a: string) => boolean`.
export function compose<A, B, C>(f: (a: A) => B, g: (b: B) => C): (a: A) => C {
  return (a) => g(f(a));
}
export function composeStringNumberBoolean() {
  return compose<string, number, boolean>(
    (a) => a.length,
    (b) => b > 0,
  );
}
export type ComposeStringNumberBooleanResult = ReturnType<typeof composeStringNumberBoolean>;

// (7) `void` return preserved as the declared shape. TS7 allows assigning a
// function with any return type to a parameter typed `() => void`, but the
// declared type of the void callback itself is still `() => void`.
export type VoidCallback = () => void;
export declare const voidCallback: VoidCallback;
export type VoidCallbackReturn = ReturnType<typeof voidCallback>;

// (8) Class-method-as-callable via `typeof <Class>.prototype.<method>`. The
// extracted method type is a callable `FunctionExpr` whose parameters and
// return follow the method declaration.
export class MethodHolder {
  greet(name: string): string {
    return `hi ${name}`;
  }
}
export type ExtractedGreetMethod = typeof MethodHolder.prototype.greet;
export type ExtractedGreetReturn = ReturnType<ExtractedGreetMethod>;
export type ExtractedGreetParams = Parameters<ExtractedGreetMethod>;

// (9) Type-parameter + string-specific overload pair. TS7 picks the FIRST
// matching overload in declaration order; the generic overload therefore
// shadows the string-specific one for ANY argument that satisfies it.
export function overloadedTypeParam<T>(x: T): T;
export function overloadedTypeParam(x: string): string;
export function overloadedTypeParam(x: any): any {
  return x;
}
export function callOverloadedTypeParamGeneric() {
  return overloadedTypeParam(42 as 42);
}
export function callOverloadedTypeParamString() {
  return overloadedTypeParam("hello");
}
export type OverloadedGenericResult = ReturnType<typeof callOverloadedTypeParamGeneric>;
export type OverloadedStringResult = ReturnType<typeof callOverloadedTypeParamString>;

// (10) Constraint-bound generic identity. `T extends string` constrains the
// type parameter; passing a string literal infers `T` to that literal type
// (the constraint widens `T`'s upper bound but does not widen its inferred
// value when the argument is `as const`).
export function constrainedIdentity<T extends string>(x: T): T {
  return x;
}
export function callConstrainedIdentity() {
  return constrainedIdentity("constrained" as const);
}
export type ConstrainedIdentityResult = ReturnType<typeof callConstrainedIdentity>;
"#;

/// The workspace-file set the function_advanced rows upsert.
#[allow(dead_code)]
const FUNCTION_ADVANCED_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/function_advanced.ts",
    source: FUNCTION_ADVANCED_SOURCE,
}];

/// Vendored source bytes of `/fixtures/branded_types.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/branded_types.ts`.
#[allow(dead_code)]
pub(crate) const BRANDED_TYPES_SOURCE: &str = r#"// @ai-generated - Synthetic branded / nominal-typing typeinfo fixture.

// (1) String brand via intersection.
export type UserId = string & { readonly __brand: "UserId" };

// (2) Numeric brand via intersection.
export type Cents = number & { readonly __cents: true };

// (3) `unique symbol` brand carried in a generic wrapper.
export declare const idBrand: unique symbol;
export type IdBranded<T> = T & { readonly [idBrand]: T };

// A concrete instantiation of the unique-symbol brand wrapper.
export type AccountId = IdBranded<string>;

// (4) Brand projection via key access — recover the literal brand tag.
export type UserIdBrandTag = UserId["__brand"];

// (5) Phantom type — two-parameter brand carrier.
export type Phantom<P, T> = T & { readonly __phantom: P };
export type EmailString = Phantom<"email", string>;

// (6) Branded type guard. `narrowUserId` produces a narrowed `UserId` after
// the guard succeeds; we resolve the alias of the unique return path to keep
// the test surface deterministic.
export declare function isUserId(x: string): x is UserId;
export function narrowUserId(value: string): UserId | undefined {
  if (isUserId(value)) {
    return value;
  }
  return undefined;
}
export type NarrowedUserId = ReturnType<typeof narrowUserId>;

// (7) Numeric brand tag projection — parallel to (4), recovers a
// boolean-literal brand tag instead of a string-literal one.
export type CentsBrandTag = Cents["__cents"];

// (8) Symbol-key value projection — recovers the value at the unique-symbol
// brand slot of a concretely-instantiated branded wrapper.
export type AccountIdBrandValue = AccountId[typeof idBrand];

// (9) Double-brand intersection — combines a string brand with a numeric
// brand. The primitive carriers (`string` and `number`) are disjoint at the
// structural level.
export type UserIdCentsBoth = UserId & Cents;
"#;

/// The workspace-file set the branded_types rows upsert.
#[allow(dead_code)]
const BRANDED_TYPES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/branded_types.ts",
    source: BRANDED_TYPES_SOURCE,
}];

/// Vendored source bytes of `/fixtures/decorators.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/decorators.ts`.
#[allow(dead_code)]
pub(crate) const DECORATORS_SOURCE: &str = r#"// @ai-generated - Synthetic TS7 (TC39 proposal-decorators) decorators typeinfo fixture.
//
// Covers identity-shaped class / method / field / accessor decorators, a
// decorator factory (a function returning a decorator), and a decorator
// that reads `ctx.metadata` (`Symbol.metadata`). All decorators below are
// identity in their effect — they return the original target — so the
// post-decoration class shape stays equal to the pre-decoration class
// shape. The tests assert that the type system preserves the published
// shape (including return-type inference for methods) across the
// decoration.

// 1. Class decorator: identity. Receives the constructor + ClassDecoratorContext,
// returns the constructor unchanged.
export function logged<T extends new (...args: any[]) => any>(
  ctor: T,
  _ctx: ClassDecoratorContext,
): T {
  return ctor;
}

@logged
export class LoggedItem {
  id: string = "";
  label(): string {
    return "label";
  }
}

export type LoggedItemInstance = InstanceType<typeof LoggedItem>;

// 2. Method decorator: identity. Receives the method + ClassMethodDecoratorContext.
export function bound<T extends (this: any, ...args: any[]) => any>(
  method: T,
  _ctx: ClassMethodDecoratorContext,
): T {
  return method;
}

export class MethodHost {
  @bound
  tag(): "tag" {
    return "tag";
  }
}

export type MethodHostInstance = InstanceType<typeof MethodHost>;
export type MethodHostTagReturn = ReturnType<MethodHost["tag"]>;

// 3. Field decorator with initializer. Receives `undefined` + ClassFieldDecoratorContext,
// returns an initializer function `(initial) => transformed`. Identity here:
// the returned function passes the initial value through unchanged.
export function tracked<This, Value>(
  _value: undefined,
  _ctx: ClassFieldDecoratorContext<This, Value>,
): (this: This, initial: Value) => Value {
  return function (this: This, initial: Value): Value {
    return initial;
  };
}

export class FieldHost {
  @tracked
  count: number = 0;
}

export type FieldHostInstance = InstanceType<typeof FieldHost>;

// 4. Accessor decorator: identity. The `accessor` keyword produces a synthesised
// pair of getter/setter; the decorator receives that pair via
// ClassAccessorDecoratorContext and returns it unchanged.
export function readonlyGet<This, Value>(
  target: ClassAccessorDecoratorTarget<This, Value>,
  _ctx: ClassAccessorDecoratorContext<This, Value>,
): ClassAccessorDecoratorTarget<This, Value> {
  return target;
}

export class AccessorHost {
  @readonlyGet
  accessor visible: string = "";
}

export type AccessorHostInstance = InstanceType<typeof AccessorHost>;

// 5. Decorator factory: a function `withTag` that captures its `tag` argument
// in a closure and returns a class decorator. The factory's parameter `tag`
// must stay the literal type "v1" passed at the call site (no widening).
export function withTag(tag: string) {
  return function <T extends new (...args: any[]) => any>(ctor: T, _ctx: ClassDecoratorContext): T {
    void tag;
    return ctor;
  };
}

@withTag("v1")
export class FactoryDecorated {
  payload: string = "";
}

export type FactoryDecoratedInstance = InstanceType<typeof FactoryDecorated>;

// 6. Decorator that reads `ctx.metadata` (`Symbol.metadata`).
// Reading the metadata bag inside the decorator must not change the class's
// public structural shape — the published instance type remains equal to
// what it would be without the decorator.
export function metadataReader<T extends new (...args: any[]) => any>(
  ctor: T,
  ctx: ClassDecoratorContext,
): T {
  // Touch ctx.metadata so the property is statically referenced. The decorator
  // does not mutate `ctor`, so the class's instance type is unchanged.
  void ctx.metadata;
  return ctor;
}

@metadataReader
export class MetadataAware {
  ready: boolean = false;
  describe(): "ready" | "pending" {
    return this.ready ? "ready" : "pending";
  }
}

export type MetadataAwareInstance = InstanceType<typeof MetadataAware>;
export type MetadataAwareDescribeReturn = ReturnType<MetadataAware["describe"]>;

// 7. Decorator factory with a `const`-modified type parameter capturing a
// literal at the call site. `withConstTag<const Tag extends string>(_tag)`
// infers `Tag = "v2"` from the call `@withConstTag("v2")`. The returned
// decorator is structurally identity (it ignores the captured `Tag` from
// the closure for type-shape purposes), so the decorated class's instance
// shape equals the bare class shape `{ visible: string }`.
export function withConstTag<const Tag extends string>(_tag: Tag) {
  return function <T extends new (...args: any[]) => any>(ctor: T, _ctx: ClassDecoratorContext): T {
    return ctor;
  };
}

@withConstTag("v2")
export class ConstTaggedClass {
  public visible: string = "";
}

export type ConstTaggedInstance = InstanceType<typeof ConstTaggedClass>;

// 8. Method decorator that invokes `ctx.addInitializer` (a `ClassMethodDecoratorContext`
// hook scheduled to run once per instance at construction). The hook is a
// runtime side-effect — it does NOT modify the method's type. The decorator
// otherwise returns the method unchanged. The published instance shape is
// therefore `{ ping(): "pong" }`.
export function methodWithInit<T extends (this: any, ...args: any[]) => any>(
  method: T,
  ctx: ClassMethodDecoratorContext,
): T {
  ctx.addInitializer(function () {
    // identity initializer
  });
  return method;
}

export class InitDecoratedClass {
  @methodWithInit
  ping() {
    return "pong" as const;
  }
}

export type InitDecoratedInstance = InstanceType<typeof InitDecoratedClass>;

// 9. Accessor decorator that returns a `ClassAccessorDecoratorTarget<unknown, T>`
// (the same shape it received). `accessor count: number` synthesises a
// getter/setter pair whose public surface is the property `count: number`.
// The decorator is type-level identity, so the published instance type
// remains `{ count: number }`.
export function trackedAccessor<T>(
  target: ClassAccessorDecoratorTarget<unknown, T>,
  _ctx: ClassAccessorDecoratorContext<unknown, T>,
): ClassAccessorDecoratorTarget<unknown, T> {
  return target;
}

export class AccessorTransformedClass {
  @trackedAccessor accessor count: number = 0;
}

export type AccessorTransformedInstance = InstanceType<typeof AccessorTransformedClass>;
"#;

/// The workspace-file set the decorators rows upsert.
#[allow(dead_code)]
const DECORATORS_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/decorators.ts",
    source: DECORATORS_SOURCE,
}];

/// Vendored source bytes of `/fixtures/substitution_types.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/substitution_types.ts`.
#[allow(dead_code)]
pub(crate) const SUBSTITUTION_TYPES_SOURCE: &str = r#"// @ai-generated - substitution-type scenarios.
//
// Each fn pins one TS7 emission for a substitution-type behaviour —
// the internal "T narrowed to T & U" mechanism TS uses to keep
// generic identity while flowing type guards through method calls,
// destructures, conditional types, asserts predicates, and the
// `in`-operator. ReturnType<typeof fn> encodes the joined narrowing.
//
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 surprises (NOT fixture bugs):
//   * Sb01: With T unspecified, `ReturnType` resolves T to `unknown`.
//     The if-branch's `T & string` collapses to `string` when T=unknown,
//     and the joined `string | unknown` collapses to `unknown`.
//   * Sb04: The RAW return annotation is `T`, but `ReturnType<typeof f>`
//     with no type args resolves T to `unknown`.
//   * Sb06: After `x = (1 as unknown) as T`, the substitution is
//     UN-NARROWED (assignment to a wider site widens). Return goes
//     back to plain T -> unknown.
//   * Sb08: `IfStr<unknown>` does NOT distribute to `"yes" | "no"`.
//     `unknown extends string` is `false` (unknown is NOT a subtype of
//     string), so the conditional resolves immediately to the false-arm:
//     `"no"`. Distribution requires the test position to be a NAKED
//     union type parameter — `unknown` is not a union, so no
//     distribution happens.
//   * Sb14: A default type argument (`<T = string>`) does NOT apply
//     inside `ReturnType<typeof fn>` — the bare function type still
//     leaves T unresolved, which `ReturnType` then collapses to
//     `unknown`. Defaults only apply at value-position call sites.
//   * Sb15: Recursive self-calls are NOT a substitution event. The
//     declared return type `T` stays as `T` -> `unknown`.

// ----- 1) Bare narrowing of generic ------------------------------------
// if-branch: T & string (substitution). else: T. With T=unknown, the
// if-branch collapses to string and the joined return is `string | unknown`
// = unknown.
export function sb01<T>(x: T) {
  if (typeof x === "string") return x;
  return x;
}
export type Sb01Result = ReturnType<typeof sb01>;

// ----- 2) Narrowing in a constrained generic ---------------------------
// if-branch: substitution `T & string` -> `.toUpperCase()` returns string.
// else: T (constrained to `string | number`). Joined: `string | (string | number)`
// = `string | number`.
export function sb02<T extends string | number>(x: T) {
  if (typeof x === "string") return x.toUpperCase();
  return x;
}
export type Sb02Result = ReturnType<typeof sb02>;

// ----- 3) Substitution survives method calls ---------------------------
// `x.toUpperCase()` on T-extends-string returns the apparent type `string`.
// Substitution is preserved across the method call. Joined: string.
export function sb03<T extends string>(x: T) {
  const u = x.toUpperCase();
  return u;
}
export type Sb03Result = ReturnType<typeof sb03>;

// ----- 4) Narrowed substitution to return position ---------------------
// RAW return annotation is `T`. `ReturnType<typeof fn>` with no type args
// resolves T to `unknown`. The narrowed `T & string` in the if-branch is
// returned as `T` at the return position.
export function sb04<T>(x: T): T {
  if (typeof x === "string") return x;
  throw 0;
}
export type Sb04Result = ReturnType<typeof sb04>;

// ----- 5) Compound narrowing via typeof && instanceof ------------------
// `typeof x === "object" && x instanceof Date` narrows x to Date.
// `x.getTime()` returns number. Joined: number.
export function sb05<T>(x: T) {
  if (typeof x === "object" && x instanceof Date) return x.getTime();
  throw 0;
}
export type Sb05Result = ReturnType<typeof sb05>;

// ----- 6) Narrowing widens after re-assignment -------------------------
// Inside the if-branch x is initially `T & string`. After
// `x = (1 as unknown) as T` the substitution is removed (TS un-narrows
// on assignment to a wider site). Return is plain T -> unknown.
export function sb06<T>(x: T) {
  if (typeof x === "string") {
    x = 1 as unknown as T;
    return x;
  }
  throw 0;
}
export type Sb06Result = ReturnType<typeof sb06>;

// ----- 7) Constraint-flow apparent type --------------------------------
// `x.len` on T-extends-{len:number} reads through the constraint's
// apparent type. Returns number.
export function sb07<T extends { len: number }>(x: T) {
  return x.len;
}
export type Sb07Result = ReturnType<typeof sb07>;

// ----- 8) Generic in conditional position retains identity -------------
// `IfStr<T>` with T=unknown does NOT distribute (unknown is NOT a naked
// union type parameter). `unknown extends string` is false, so the
// conditional resolves immediately to the false-arm: "no".
export type IfStr<T> = T extends string ? "yes" : "no";
export function sb08<T>() {
  return null as unknown as IfStr<T>;
}
export type Sb08Result = ReturnType<typeof sb08>;

// ----- 9) `asserts x is string` on generic -----------------------------
// After the assert, x is `T & string`. With T=unknown via ReturnType,
// `unknown & string` collapses to `string`. Joined: string.
export function assertIsString(x: any): asserts x is string {}
export function sb09<T>(x: T) {
  assertIsString(x);
  return x;
}
export type Sb09Result = ReturnType<typeof sb09>;

// ----- 10) `x is T` predicate on generic -------------------------------
// After `isFoo<T>(x)` the variable x narrows to T. ReturnType resolves T
// to unknown. Joined: unknown.
export function isFoo<T>(x: unknown): x is T {
  return true;
}
export function sb10<T>(x: unknown) {
  if (isFoo<T>(x)) return x;
  throw 0;
}
export type Sb10Result = ReturnType<typeof sb10>;

// ----- 11) Generic narrowed via `in` operator --------------------------
// `"a" in x` narrows T's apparent type to the `{ a: 1 }` arm. Else
// returns the `{ b: 2 }` arm. Joined: 1 | 2.
export function sb11<T extends { a: 1 } | { b: 2 }>(x: T) {
  if ("a" in x) return x.a;
  return x.b;
}
export type Sb11Result = ReturnType<typeof sb11>;

// ----- 12) Truthiness narrowing on `T extends string | undefined` ------
// Truthy guard removes `undefined`. With the constraint `string | undefined`,
// truthy reduces to `string`. Joined: string.
export function sb12<T extends string | undefined>(x: T) {
  if (x) return x;
  throw 0;
}
export type Sb12Result = ReturnType<typeof sb12>;

// ----- 13) Substitution carried across destructure ---------------------
// Destructuring `{ val }` from T-extends-{val:number} reads `val` as the
// constraint's apparent property type: number. Joined: number.
export function sb13<T extends { val: number }>(x: T) {
  const { val } = x;
  return val;
}
export type Sb13Result = ReturnType<typeof sb13>;

// ----- 14) Default type arg with narrowing -----------------------------
// SURPRISE: `<T = string>` default does NOT apply inside `ReturnType<typeof fn>`.
// The bare function type is still unparameterised; ReturnType resolves T
// to `unknown` (not the default). Joined: unknown.
export function sb14<T = string>(x: T) {
  if (typeof x === "string") return x;
  return x;
}
export type Sb14Result = ReturnType<typeof sb14>;

// ----- 15) Recursive generic substitution ------------------------------
// Self-recursion `f(x)` is NOT a substitution event. Declared return
// type T stays as T -> unknown.
export function sb15<T>(x: T): T {
  return sb15(x);
}
export type Sb15Result = ReturnType<typeof sb15>;
"#;

/// The workspace-file set the substitution_types rows upsert.
#[allow(dead_code)]
const SUBSTITUTION_TYPES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/substitution_types.ts",
    source: SUBSTITUTION_TYPES_SOURCE,
}];

/// The vendored source bytes of `/fixtures/typescript-rules.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `oracle_query_specs_guard` asserts byte-identity with `fixtures/typescript_rules.ts`.
#[allow(dead_code)]
pub(crate) const TYPESCRIPT_RULES_SOURCE: &str = r#"// @ai-generated - Synthetic TypeScript type-system rules fixture.

export type LiteralAndPrimitiveSurface = {
  stringLiteral: "ready";
  numberLiteral: 42;
  booleanLiteral: true;
  stringValue: string;
  numberValue: number;
  booleanValue: boolean;
  symbolValue: symbol;
  bigintValue: bigint;
  nullValue: null;
  undefinedValue: undefined;
  unknownValue: unknown;
  anyValue: any;
  neverValue: never;
};

export type MethodAndIndexSurface = {
  readonly id: string;
  label?: string;
  method?: (input: string, count?: number) => boolean;
  [key: string]:
    | string
    | number
    | boolean
    | undefined
    | ((input: string, count?: number) => boolean);
};

export type TupleRules = [name: string, count?: number, ...flags: boolean[]];

export type ReadonlyTupleRules = readonly [mode: "view", values: readonly number[]];

export type FunctionRules = (
  item: { id: string },
  ...flags: boolean[]
) => { id: string; flags: boolean[] };

export type RecordLiteralKeys = Record<"alpha" | "beta", number>;

export type MappedModifierRules<T> = {
  readonly [K in keyof T]-?: T[K];
};

export type MappedModifierSurface = MappedModifierRules<{
  id?: string;
  count?: number;
}>;

export type UnionObjectRules =
  | { kind: "a"; a: string; shared: boolean }
  | { kind: "b"; b: number; shared: boolean };

export type IntersectionObjectRules = { id: string } & { count?: number } & {
  readonly ready: boolean;
};

export interface KeySource {
  id: string;
  count?: number;
  nested: {
    value: string;
  };
}

export type KeyOfRules = keyof KeySource;
export type IndexedRules = KeySource["nested"]["value"];

export type ConditionalDistributive<T> = T extends string ? { text: T } : { other: T };
export type ConditionalDistributedRules = ConditionalDistributive<"a" | 1>;

export type ConditionalNonDistributive<T> = [T] extends [string] ? { text: T } : { other: T };
export type ConditionalNonDistributedRules = ConditionalNonDistributive<"a" | 1>;

export type ConstructorLike = new (id: string) => { id: string; ready: boolean };
export type ConstructorParamsRules = ConstructorParameters<ConstructorLike>;
export type InstanceRules = InstanceType<ConstructorLike>;

export class ClassRules {
  id: string;
  constructor(id: string);
  method(count: number): string;
}
export type ClassInstanceRules = InstanceType<typeof ClassRules>;
export type ClassConstructorParamsRules = ConstructorParameters<typeof ClassRules>;

export const literalConfig = {
  mode: "view",
  nested: {
    value: 1,
  },
} as const;
export type TypeOfConstRules = typeof literalConfig;
export type TypeOfConstNestedValue = typeof literalConfig.nested.value;

export type AwaitedRules = Awaited<Promise<Promise<{ done: true }>>>;

export type TemplateIntrinsicRules = `on${Capitalize<"submit" | "cancel">}`;

export type KeyRemapExcludeRules<T> = {
  [K in keyof T as K extends "internal" ? never : `public:${K & string}`]: T[K];
};
export type KeyRemapExcludeSurface = KeyRemapExcludeRules<{
  id: string;
  internal: boolean;
  count: number;
}>;
"#;

/// The vendored source bytes of `/fixtures/deep-path.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `oracle_query_specs_guard` asserts byte-identity with `fixtures/deep_path.ts`.
#[allow(dead_code)]
pub(crate) const DEEP_PATH_SOURCE: &str = r#"// @ai-generated - Synthetic deep indexed-access typeinfo fixture.

export type TerminalPayload = {
  id: string;
  priority: 1 | 2 | 3;
};

export type HeavySibling00 = {
  ignored00: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling01 = {
  ignored01: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling02 = {
  ignored02: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling03 = {
  ignored03: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling04 = {
  ignored04: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling05 = {
  ignored05: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling06 = {
  ignored06: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling07 = {
  ignored07: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling08 = {
  ignored08: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling09 = {
  ignored09: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling10 = {
  ignored10: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling11 = {
  ignored11: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling12 = {
  ignored12: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling13 = {
  ignored13: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling14 = {
  ignored14: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type HeavySibling15 = {
  ignored15: {
    label: string;
    values: Array<{ id: string; score: number; nested: Record<string, string[]> }>;
  };
};

export type Layer00<T> = { target: T; sibling00?: HeavySibling00 };
export type Layer01<T> = { level00: Layer00<T>; sibling01?: HeavySibling01 };
export type Layer02<T> = { level01: Layer01<T>; sibling02?: HeavySibling02 };
export type Layer03<T> = { level02: Layer02<T>; sibling03?: HeavySibling03 };
export type Layer04<T> = { level03: Layer03<T>; sibling04?: HeavySibling04 };
export type Layer05<T> = { level04: Layer04<T>; sibling05?: HeavySibling05 };
export type Layer06<T> = { level05: Layer05<T>; sibling06?: HeavySibling06 };
export type Layer07<T> = { level06: Layer06<T>; sibling07?: HeavySibling07 };
export type Layer08<T> = { level07: Layer07<T>; sibling08?: HeavySibling08 };
export type Layer09<T> = { level08: Layer08<T>; sibling09?: HeavySibling09 };
export type Layer10<T> = { level09: Layer09<T>; sibling10?: HeavySibling10 };
export type Layer11<T> = { level10: Layer10<T>; sibling11?: HeavySibling11 };
export type Layer12<T> = { level11: Layer11<T>; sibling12?: HeavySibling12 };
export type Layer13<T> = { level12: Layer12<T>; sibling13?: HeavySibling13 };
export type Layer14<T> = { level13: Layer13<T>; sibling14?: HeavySibling14 };
export type Layer15<T> = { level14: Layer14<T>; sibling15?: HeavySibling15 };
export type DeepRoot = Layer15<TerminalPayload>;
export type DeepProjectedTarget =
  DeepRoot["level14"]["level13"]["level12"]["level11"]["level10"]["level09"]["level08"]["level07"]["level06"]["level05"]["level04"]["level03"]["level02"]["level01"]["level00"]["target"];
"#;

/// The workspace-file set the two `typescript_rules.rs` carve-out rows upsert.
#[allow(dead_code)]
const TYPESCRIPT_RULES_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/typescript-rules.ts",
    source: TYPESCRIPT_RULES_SOURCE,
}];

/// The workspace-file set the `deep_path.rs` carve-out row upserts.
#[allow(dead_code)]
const DEEP_PATH_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: "/fixtures/deep-path.ts",
    source: DEEP_PATH_SOURCE,
}];
