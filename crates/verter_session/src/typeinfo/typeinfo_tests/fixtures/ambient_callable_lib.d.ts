// The callable half of the TypeScript standard library, vendored verbatim in
// shape from `lib.es5.d.ts`: the two interfaces a call-signature-bearing type
// widens to when a member is looked up on it. `Function` is the base surface;
// `CallableFunction` is the strict-mode surface whose `call` / `apply` carry
// the receiver-and-argument type parameters.
interface Function {
  apply(this: Function, thisArg: any, argArray?: any): any;
  call(this: Function, thisArg: any, ...argArray: any[]): any;
  bind(this: Function, thisArg: any, ...argArray: any[]): any;
  toString(): string;
  prototype: any;
  readonly length: number;
  arguments: any;
  caller: Function;
}

interface CallableFunction extends Function {
  call<T, R>(this: (this: T) => R, thisArg: T): R;
  call<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, ...args: A): R;
  apply<T, R>(this: (this: T) => R, thisArg: T): R;
  apply<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, args: A): R;
  bind<T>(this: T, thisArg: ThisParameterType<T>): OmitThisParameter<T>;
}

type ThisParameterType<T> = T extends (this: infer U, ...args: never) => any ? U : unknown;

type OmitThisParameter<T> = unknown extends ThisParameterType<T>
  ? T
  : T extends (...args: infer A) => infer R
    ? (...args: A) => R
    : T;
