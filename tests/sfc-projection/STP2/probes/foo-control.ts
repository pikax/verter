/** Syntax control from STP2.2. Construction is typechecking-only. */
export declare class Foo<T = unknown> {
  constructor(props?: { test: T });
  readonly value: T;
}

export declare class ConstrainedFoo<T extends string | number = number> {
  constructor(props?: { test: T });
  readonly value: T;
}
