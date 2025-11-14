declare function assertType<T>(value: T): T;
declare function assertNever<T extends never>(): T;
declare function assertEqual<T, U extends T>(): U;

declare function describe(e: string, fn: () => void): void;
declare function it(e: string, fn: () => void): void;
