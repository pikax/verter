export declare const UniqueKey: unique symbol;

/**
 * A utility type that patches a type `T` with a hidden property keyed by a unique symbol.
 * This is useful for attaching metadata to types without affecting their public interface.
 */
export type PatchHidden<T, E> = { [UniqueKey]?: E } & T;

/**
 * A utility type that extracts the hidden property from a type `T` if it exists.
 * If the hidden property does not exist, it returns the default type `R`.
 */
export type ExtractHidden<T, R = never> = T extends {
  [UniqueKey]?: infer U;
}
  ? U 
  : R;

/**
 * A utility type that converts a function type representing event emissions
 * into an object type mapping event names to their argument tuples.
 */
export type FunctionToObject<T> = T extends (
  e: infer X extends string,
  ...args: infer A
) => void
  ? // using PatchHidden here does not work
    { [UniqueKey]?: { [K in X]: A } } & ((e: X, ...args: A) => void)
  : never;

/**
 * A utility type that converts an intersection of function types
 * representing event emissions into an object type mapping event names to their argument tuples.
 */
export type IntersectionFunctionToObject<T> = T extends FunctionToObject<T> &
  infer N
  ? FunctionToObject<T> & IntersectionFunctionToObject<N>
  : {};

/**
 * A utility type that makes properties of type `T` that can be `undefined` optional.
 * Properties that cannot be `undefined` remain required.
 */
export type PartialUndefined<T> = {
  [P in keyof T]: undefined extends T[P] ? P : never;
}[keyof T] extends infer U extends keyof T
  ? Omit<T, U> & Partial<Pick<T, U>>
  : T;


//  --- External Sources ---

// Source - https://stackoverflow.com/a
// Posted by jcalz, modified by community. See post 'Timeline' for change history
// Retrieved 2025-11-14, License - CC BY-SA 4.0

export type UnionToIntersection<U> = (
  U extends any ? (x: U) => void : never
) extends (x: infer I) => void
  ? I
  : never;
