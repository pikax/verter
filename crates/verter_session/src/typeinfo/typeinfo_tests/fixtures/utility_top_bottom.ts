// @ai-generated - Synthetic top/bottom-type utility fixture.
//
// Codifies TS7's behaviour for built-in utilities applied to top/bottom
// inputs (`any` / `unknown` / `never` / `null` / `undefined` / `void`).
// These are the "catches you when regular objects pass but degenerate
// inputs blow up" cases for the resolver's utility dispatch.
//
// Each alias is consumed through a corresponding Rust test in
// `utility_top_bottom.rs`.

// ============================================================
// ReturnType matrix
// ============================================================

// TS7: `any` (the conditional distributes over `any`, both branches
// contribute, and the merged result collapses to `any`).
export type Utb01ReturnTypeOfAny = ReturnType<any>;

// TS7: never (cannot extract a return from a bottom-typed callable)
export type Utb02ReturnTypeOfNever = ReturnType<never>;

// TS7: any
export type Utb03ReturnTypeAnyArrow = ReturnType<() => any>;

// TS7: never
export type Utb04ReturnTypeNeverArrow = ReturnType<() => never>;

// TS7: unknown
export type Utb05ReturnTypeUnknownArrow = ReturnType<() => unknown>;

// TS7: void
export type Utb06ReturnTypeVoidArrow = ReturnType<() => void>;

// ============================================================
// Parameters matrix
// ============================================================

// TS7: `unknown[]` — when T is `any`, the inferred `infer P` resolves
// against the constraint `(...args: any) => any`, yielding `unknown[]`
// (NOT `any` and NOT `never`). This is one of the trap cases.
export type Utb07ParametersOfAny = Parameters<any>;

// TS7: never
export type Utb08ParametersOfNever = Parameters<never>;

// TS7: [x: any]
export type Utb09ParametersAnyArg = Parameters<(x: any) => void>;

// TS7: [x: never]
export type Utb10ParametersNeverArg = Parameters<(x: never) => void>;

// ============================================================
// ConstructorParameters / InstanceType
// ============================================================

// TS7: `unknown[]` — like `Parameters<any>`, `ConstructorParameters<any>`
// reduces to the constraint's inferred tuple = `unknown[]`.
export type Utb11ConstructorParametersAny = ConstructorParameters<any>;

// TS7: any
export type Utb12InstanceTypeAny = InstanceType<any>;

// TS7: any[]
export type Utb13ConstructorParametersAnyCtor = ConstructorParameters<new (...args: any[]) => any>;

// ============================================================
// Awaited matrix
// ============================================================

// TS7: any
export type Utb14AwaitedAny = Awaited<any>;

// TS7: unknown
export type Utb15AwaitedUnknown = Awaited<unknown>;

// TS7: never
export type Utb16AwaitedNever = Awaited<never>;

// TS7: null
export type Utb17AwaitedNull = Awaited<null>;

// TS7: undefined
export type Utb18AwaitedUndefined = Awaited<undefined>;

// TS7: string (Awaited recursively unwraps nested Promises)
export type Utb19AwaitedNestedPromise = Awaited<Promise<Promise<string>>>;

// ============================================================
// NonNullable matrix
// ============================================================

// TS7: any
export type Utb20NonNullableAny = NonNullable<any>;

// TS7: {} (NonNullable<unknown> reduces to the empty-object base)
export type Utb21NonNullableUnknown = NonNullable<unknown>;

// TS7: never
export type Utb22NonNullableNever = NonNullable<never>;

// TS7: never (every constituent of the input is null or undefined)
export type Utb23NonNullableNullableOnly = NonNullable<null | undefined>;

// ============================================================
// Extract / Exclude matrix
// ============================================================

// TS7: any
export type Utb24ExtractAnyAgainstString = Extract<any, string>;

// TS7: any
export type Utb25ExcludeAnyAgainstString = Exclude<any, string>;

// TS7: never (distributing over `never` collapses)
export type Utb26ExtractNeverAgainstString = Extract<never, string>;

// TS7: never
export type Utb27ExcludeNeverAgainstString = Exclude<never, string>;

// TS7: never (`unknown extends string` is false, so `T extends U ? T : never`
// collapses to `never`).
export type Utb28ExtractUnknownAgainstString = Extract<unknown, string>;

// TS7: unknown
export type Utb29ExcludeUnknownAgainstString = Exclude<unknown, string>;
