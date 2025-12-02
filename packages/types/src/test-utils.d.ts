// Type assertion helpers for tests - automatically available in .spec.ts files

declare global {
  // Assert that two types are exactly equal
  function assertType<T>(value: T): void;

  // Assert that a type is never
  function assertNever<T extends never>(): T;
}

export {};
