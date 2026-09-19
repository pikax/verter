/** Ordered-operation and split-witness models for STP3. Typechecking-only. */

export type Accumulate<A, B> = A & B;

export type FirstListener = (payload: { from: "first" }) => void;
export type SecondListener = (payload: { from: "second" }) => void;
export type AccumulatedListeners = Accumulate<FirstListener, SecondListener>;
export type LastWriteWinsListeners = SecondListener;

export declare function splitUse<T = unknown, U = unknown>(
  rows?: T[],
): (project: (row: T) => U) => { t: T; value: U };

export declare function startRows<T>(rows: T[]): {
  thenProject<U>(project: (row: T) => U): { value: U };
};

export declare function startProject<U>(project: (row: unknown) => U): {
  thenRows<T>(rows: T[]): { value: U };
};

export declare class SharedSpecialization {
  constructor(props?: { rows?: string[]; project?: (row: string) => string });
  readonly value: string;
}
