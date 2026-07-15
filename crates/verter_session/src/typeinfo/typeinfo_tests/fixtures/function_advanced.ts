// @ai-generated - Synthetic advanced-function typeinfo fixture.

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
