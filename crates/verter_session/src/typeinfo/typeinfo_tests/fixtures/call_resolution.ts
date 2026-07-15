// @ai-generated - Synthetic call-site resolution fixture.
//
// This fixture is the COMPLEMENT of function_advanced.ts. Whereas that file
// characterises the SHAPES of function and constructor types (signatures,
// `this`-typing, hybrid call+construct interfaces, generic function aliases),
// THIS file characterises CALL-SITE BEHAVIOUR: overload PICKING at concrete
// argument lists, generic INFERENCE from positional arguments and callback
// signatures, `this`-receiver binding at method call, extracted method
// invocation through `Function.prototype.call`, and constructor overload
// picking via `ConstructorParameters<>`.
//
// Every callable below funnels through a synthetic wrapper function whose
// `ReturnType<typeof wrapper>` is the assertion surface — the resolver only
// needs to project the wrapper's inferred return.

// (1) Overload selection — contextual callback return picks the first overload
// whose callback return type matches the contextual literal.
export declare function pick<T extends string>(value: T, cb: (v: T) => "ok"): T;
export declare function pick<T extends number>(value: T, cb: (v: T) => "nope"): T;
export function callPickContextual() {
  return pick("hello", (v) => "ok");
}
export type ContextualPickResult = ReturnType<typeof callPickContextual>;

// (2) Overload selection — optional vs. rest parameter ordering. Each call
// picks the FIRST matching declared overload.
export declare function call(a: string): "with-a";
export declare function call(a: string, b?: number): "with-b";
export declare function call(...rest: string[]): "rest";
export function callOptional1() {
  return call("x");
}
export function callOptional2() {
  return call("x", 1);
}
export function callOptional3() {
  return call("x", "y", "z");
}
export type CallOptional1Result = ReturnType<typeof callOptional1>;
export type CallOptional2Result = ReturnType<typeof callOptional2>;
export type CallOptional3Result = ReturnType<typeof callOptional3>;

// (3) Union argument does NOT distribute through an overload set. TS7
// requires a SINGLE overload to satisfy the union — it does NOT pick one
// overload per arm and union the returns. Without an explicit
// union-accepting overload, the call `lookup("a" | "b")` fails with
// "No overload matches this call". To characterise this contract WITHOUT
// silencing the diagnostic, the fixture declares a third overload
// `(key: "a" | "b"): "value-a" | "value-b"` — that is the overload TS
// picks for the union-keyed call. Direct literal calls still pick the
// first matching declared overload (`"a"` -> first, `"b"` -> second),
// proving the third overload is reached only when the argument literally
// is the union.
export declare function lookup(key: "a"): "value-a";
export declare function lookup(key: "b"): "value-b";
export declare function lookup(key: "a" | "b"): "value-a" | "value-b";
export function lookupUnion(key: "a" | "b") {
  return lookup(key);
}
export function lookupSpecificA() {
  return lookup("a");
}
export function lookupSpecificB() {
  return lookup("b");
}
export type LookupUnionResult = ReturnType<typeof lookupUnion>;
export type LookupSpecificAResult = ReturnType<typeof lookupSpecificA>;
export type LookupSpecificBResult = ReturnType<typeof lookupSpecificB>;

// (4) Generic inference from a positional argument with callback contextual
// binding. `T` is inferred from the SECOND parameter; the callback's parameter
// type is the contextual binding for `T`.
export declare function withCallback<T>(cb: (item: T) => unknown, item: T): T;
export function inferFromCallbackParam() {
  return withCallback((item) => item, "literal" as const);
}
export type CallbackParamInfer = ReturnType<typeof inferFromCallbackParam>;

// (5) Generic inference from a callback return type. `T` is inferred from
// the callback's return.
export declare function lift<T>(cb: () => T): { value: T };
export function inferFromCallbackReturn() {
  return lift(() => 42 as const);
}
export type CallbackReturnInfer = ReturnType<typeof inferFromCallbackReturn>;

// (6) Generic inference from an object literal argument. The literal carries
// EXCESS properties beyond the `mode` constraint; TS infers `T` as the literal
// type INCLUDING the excess property.
export declare function configure<T extends { mode: string }>(config: T): T;
export function inferFromObjectLiteral() {
  return configure({ mode: "active", debug: true });
}
export type ObjectLiteralInfer = ReturnType<typeof inferFromObjectLiteral>;

// (7) `this`-receiver call. The `this: { data: string }` annotation
// constrains call sites but is invisible to `Parameters<>`; ordinary method
// access binds the receiver implicitly. Return is `string`.
export const receiverObj = {
  data: "hello",
  greet(this: { data: string }, suffix: string): string {
    return this.data + suffix;
  },
};
export function callThis() {
  return receiverObj.greet("!");
}
export type ThisReceiverResult = ReturnType<typeof callThis>;

// (8) Method extracted via `Class.prototype.method` and invoked via
// `Function.prototype.call`. The extracted callable's return is preserved.
export class Greeter {
  greet(name: string): string {
    return `hi ${name}`;
  }
}
export function callExtractedMethod() {
  const m = Greeter.prototype.greet;
  return m.call(new Greeter(), "test");
}
export type ExtractedMethodResult = ReturnType<typeof callExtractedMethod>;

// (9) Constructor overloads. `ConstructorParameters<typeof Class>` selects
// the LAST declared overload.
export declare class CtorOverloaded {
  constructor(value: string);
  constructor(value: number, multiplier: number);
}
export type CtorParams1 = ConstructorParameters<typeof CtorOverloaded>;

// (10) Abstract constructor type used with `InstanceType<>`. The instance
// shape includes constructor-declared parameter properties plus abstract
// method signatures.
export abstract class AbstractBase {
  constructor(public name: string) {}
  abstract describe(): string;
}
export type AbstractInstanceShape = InstanceType<abstract new (name: string) => AbstractBase>;
