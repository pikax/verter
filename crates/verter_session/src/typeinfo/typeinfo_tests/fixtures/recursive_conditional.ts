// @ai-generated - Synthetic recursive conditional-type typeinfo fixture.

export type Flatten<T> = T extends readonly (infer U)[] ? Flatten<U> : T;

export type FlattenedThreeDeepArray = Flatten<string[][][]>;
export type FlattenedAlreadyFlat = Flatten<number>;

export type DeepReadonly<T> = T extends (...args: any) => any
  ? T
  : T extends object
    ? { readonly [K in keyof T]: DeepReadonly<T[K]> }
    : T;

export type DeepConfig = {
  outer: {
    inner: {
      flag: boolean;
      label: string;
    };
    list: string[];
  };
  scalar: number;
};
export type DeepReadonlyConfig = DeepReadonly<DeepConfig>;

export type DeepPartial<T> = T extends (...args: any) => any
  ? T
  : T extends object
    ? { [K in keyof T]?: DeepPartial<T[K]> }
    : T;

export type DeepPartialConfig = DeepPartial<{
  scalar: number;
  nested: { name: string; count: number };
}>;

export type AwaitedRecursive<T> = T extends Promise<infer Inner> ? AwaitedRecursive<Inner> : T;
export type DoubleAwaited = AwaitedRecursive<Promise<Promise<{ id: string }>>>;
