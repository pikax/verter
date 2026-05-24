// @ai-generated - Synthetic modern-TS-features typeinfo fixture.
//
// Covers TS7 feature surface that NoInfer and `const T` don't already
// characterise:
//
//   * Variance annotations `<in T>`, `<out U>`, `<in out V>` — used by
//     interface declarations. Variance is enforced by assignability, not by
//     the structural shape; the projector should still surface the declared
//     members.
//
//   * `using` declarations + `Symbol.dispose` — encoded structurally as a
//     function that returns the consumed value. The fixture declares its own
//     `DisposableLike` interface that mirrors the lib.esnext.disposable shape
//     so the surface is hermetic and does not rely on the runtime lib.
//
//   * `await using` + `Symbol.asyncDispose` — analogous to `using`, with an
//     async dispose return.
//
//   * Import attributes `import x from "..." with { type: "json" }` — encoded
//     structurally as an `as const` literal so the surface characterises what
//     an import-attribute'd JSON module would produce.
//
//   * `satisfies` operator deep widening: literal keys are preserved, but
//     inner values widen unless `as const` is applied. This captures the
//     `satisfies` ! = `as const` distinction.
//
// Where tsgo (7.0.0-dev.20260423.1) cannot or might not typecheck a true
// in-language form of the feature (real `using`, real import attributes), the
// fixture uses a SIMULATED structural encoding that produces the same surface
// the real feature would yield. Each simulated path is documented at its
// declaration so the test contract stays unambiguous.

// ---------------------------------------------------------------------------
// (1) Variance annotations
// ---------------------------------------------------------------------------
// `<out T>` — covariant; `<in T>` — contravariant; `<in out T>` — invariant.
// These are checked at assignability time, not in the structural surface.
// The projector should publish the interface members as declared.

export interface Producer<out T> {
  create(): T;
}
export interface Consumer<in T> {
  consume(value: T): void;
}
export interface Invariant<in out T> {
  transfer(value: T): T;
}

export type ProducerString = Producer<string>;
export type ConsumerNumber = Consumer<number>;
export type InvariantBoolean = Invariant<boolean>;

// ---------------------------------------------------------------------------
// (2) `using` declarations / Symbol.dispose (SIMULATED structural form)
// ---------------------------------------------------------------------------
// The real `using` lexical form requires lib.esnext.disposable in scope.
// To keep this fixture hermetic, we declare our own DisposableLike interface
// with the same shape as the lib emission and write a normal helper that
// consumes a DisposableLike and returns its `.value` field. The structural
// SURFACE the test characterises is identical to the surface that
// `using resource = makeResource(); return resource.value` would produce: the
// helper's return type is `string`.
//
// If tsgo gains real `using` support and we want a literal in-language test,
// replace `consumeDisposable` with a function whose body is:
//   using resource = makeResource();
//   return resource.value;
// — the return type characterisation does not change.

export interface DisposableLike {
  readonly value: string;
  // The real `using` keyword requires the operand to satisfy the
  // built-in `Disposable` interface which carries `[Symbol.dispose](): void`.
  // We mirror that exact key here so the `RealUsingResult` companion test
  // below typechecks against tsgo's `using` semantics. The simulated
  // `consumeDisposable` test that consumes this type via a regular `const`
  // is unaffected by the additional key.
  [Symbol.dispose](): void;
}

export declare function makeDisposable(): DisposableLike;
export function consumeDisposable(): string {
  const resource = makeDisposable();
  try {
    return resource.value;
  } finally {
    resource[Symbol.dispose]();
  }
}
export type ConsumeDisposableResult = ReturnType<typeof consumeDisposable>;

// ---------------------------------------------------------------------------
// (3) `await using` / Symbol.asyncDispose (SIMULATED structural form)
// ---------------------------------------------------------------------------
// Same hermeticity rationale as (2): declare our own AsyncDisposableLike with
// the dispose method renamed to `disposeAsync` (mirroring the runtime
// `[Symbol.asyncDispose]` slot) and write a normal async helper. The
// resolved surface for `consumeAsyncDisposable` is `Promise<number>`; the
// resolved type of `AsyncConsumeResult` is `number` after unwrap.

export interface AsyncDisposableLike {
  readonly count: number;
  disposeAsync(): Promise<void>;
}

export declare function makeAsyncDisposable(): AsyncDisposableLike;
export async function consumeAsyncDisposable(): Promise<number> {
  const resource = makeAsyncDisposable();
  try {
    return resource.count;
  } finally {
    await resource.disposeAsync();
  }
}
export type AsyncConsumeResult = Awaited<ReturnType<typeof consumeAsyncDisposable>>;

// ---------------------------------------------------------------------------
// (4) Import attributes (SIMULATED form)
// ---------------------------------------------------------------------------
// Real form:
//   import data from "./config.json" with { type: "json" };
//   type ConfigData = typeof data;
//
// tsgo may not resolve the JSON module without a real ./config.json on disk;
// we declare the imported shape inline as an `as const` literal. The
// resolved surface is identical to the real form: the imported JSON value
// would yield the same literal object shape if the JSON file held those
// exact values.

const importedJsonConfig = {
  name: "verter-fixture",
  version: 1,
} as const;
export type ImportedJsonConfig = typeof importedJsonConfig;
export type ImportedJsonName = ImportedJsonConfig["name"];

// ---------------------------------------------------------------------------
// (5) `satisfies` operator — deep widening behaviour
// ---------------------------------------------------------------------------
// `satisfies` constrains a value against a target type WITHOUT widening the
// value's type to the target. Object keys are preserved as literal keys
// (because the object expression keeps its inferred shape), but inner values
// widen UNLESS the value is `as const`-asserted. This means:
//   * `keyof typeof cfg` preserves the literal keys "a" | "b"
//   * `typeof cfg.a.count` widens to `number` (NOT the literal `1`)

export type CfgEntry = { count: number };
export type CfgShape = Record<string, CfgEntry>;
export const cfg = {
  a: { count: 1 },
  b: { count: 2 },
} satisfies CfgShape;

export type CfgKeys = keyof typeof cfg;
export type CfgValueACount = typeof cfg.a.count;

// ---------------------------------------------------------------------------
// (6) Variance annotation T substitution through Consumer.consume
// ---------------------------------------------------------------------------
// The existing variance tests prove the interface declaration is resolved
// structurally. This case exercises the type-parameter SUBSTITUTION through a
// method member: `Parameters<NumberConsumer["consume"]>` must materialise the
// labelled tuple `[value: number]` — T must be substituted from the
// variance-annotated `<in T>` parameter into the consume method's parameter,
// NOT left as the generic `T`.

export type NumberConsumer = Consumer<number>;
export type NumberConsumerParameters = Parameters<NumberConsumer["consume"]>;

// ---------------------------------------------------------------------------
// (7) `satisfies` with array literal
// ---------------------------------------------------------------------------
// TS7 contract: `typeof arrSat` resolves to `number[]`. The `satisfies
// readonly number[]` clause checks assignability but does NOT preserve the
// tuple shape — the inferred type for `[1, 2, 3]` (without `as const`) is
// `number[]`. This locks in the documented `satisfies` != `as const`
// behaviour for array literals.

export const arrSat = [1, 2, 3] satisfies readonly number[];
export type ArrSatType = typeof arrSat;

// ---------------------------------------------------------------------------
// (8) Real `using` declaration
// ---------------------------------------------------------------------------
// Companion to the existing simulated `using` form above. Exercises the real
// `using` keyword against the same DisposableLike shape. The structural
// return surface is identical to `consumeDisposable`: the helper's return
// type is `string`. Captured separately as `RealUsingResult` so the test
// contract states unambiguously what is being characterised.

export function consumeDisposableUsing(): string {
  using resource = makeDisposable();
  return resource.value;
}
export type RealUsingResult = ReturnType<typeof consumeDisposableUsing>;
