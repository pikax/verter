// @ai-generated - Synthetic variadic-tuple typeinfo fixture.

export type Head<T extends readonly unknown[]> = T extends readonly [infer H, ...unknown[]]
  ? H
  : never;
export type Tail<T extends readonly unknown[]> = T extends readonly [unknown, ...infer R] ? R : [];
export type Last<T extends readonly unknown[]> = T extends readonly [...unknown[], infer L]
  ? L
  : never;
export type Init<T extends readonly unknown[]> = T extends readonly [...infer I, unknown] ? I : [];
export type Concat<A extends readonly unknown[], B extends readonly unknown[]> = [...A, ...B];

export type SampleTuple = [1, 2, 3];

export type HeadOfSample = Head<SampleTuple>;
export type TailOfSample = Tail<SampleTuple>;
export type LastOfSample = Last<SampleTuple>;
export type InitOfSample = Init<SampleTuple>;
export type ConcatPair = Concat<[1, 2], [3, 4]>;

// Variadic in a function signature
export declare function variadic<A extends readonly unknown[], B extends readonly unknown[]>(
  a: [...A],
  b: [...B],
): [...A, ...B];

export type VariadicCallResult = ReturnType<typeof variadic<[1, 2], [3, 4]>>;
