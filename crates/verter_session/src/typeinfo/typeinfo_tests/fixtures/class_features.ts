// @ai-generated - Synthetic class-features typeinfo fixture.
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
