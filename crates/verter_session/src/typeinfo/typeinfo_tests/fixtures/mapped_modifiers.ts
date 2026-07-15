// @ai-generated - Synthetic mapped-type modifier typeinfo fixture.
//
// Exercises the `+readonly` / `-readonly` adders/removers, the `+?` / `-?`
// optional modifier toggles, an `as never` key filter, and a mapped type that
// uses a conditional value expression. All shapes are synthetic — no
// dependency on real libraries.

// ---------------------------------------------------------------------------
// (1) +readonly adder
// ---------------------------------------------------------------------------
export type MutableSource = { a: string; b: number };
export type AllReadonly<T> = { +readonly [K in keyof T]: T[K] };
export type AddReadonlyResult = AllReadonly<MutableSource>;

// ---------------------------------------------------------------------------
// (2) -readonly remover
// ---------------------------------------------------------------------------
export type ReadonlySource = { readonly a: string; readonly b: number };
export type Mutable<T> = { -readonly [K in keyof T]: T[K] };
export type MutableResult = Mutable<ReadonlySource>;

// ---------------------------------------------------------------------------
// (3) +? optional adder
// ---------------------------------------------------------------------------
export type RequiredSource = { a: string; b: number };
export type AllOptional<T> = { [K in keyof T]+?: T[K] };
export type AddOptionalResult = AllOptional<RequiredSource>;

// ---------------------------------------------------------------------------
// (4) -? optional remover — PRESENCE-ONLY
//
// `-?` is a presence modifier: it clears the optional flag. The
// optional-origin `undefined` is carried by the flag (not in the value
// slot), so clearing the flag on an OPTIONAL-origin property naturally
// yields the bare value type. `OptionalSource` uses the `?` marker so
// `AllRequired<OptionalSource>` = `{ a: string; b: number }` (bare).
//
// The companion `ExplicitUndefinedSource` proves the dual: a property whose
// `| undefined` is EXPLICIT on a REQUIRED slot is preserved by `-?` (real TS:
// `Required<{ a: string | undefined }>` = `{ a: string | undefined }`), because
// the `undefined` is part of the declared type, not an optional-origin marker.
// ---------------------------------------------------------------------------
export type OptionalSource = {
  a?: string;
  b?: number;
};
export type AllRequired<T> = { [K in keyof T]-?: T[K] };
export type RemoveOptionalResult = AllRequired<OptionalSource>;

export type ExplicitUndefinedSource = {
  a: string | undefined;
};
export type RequiredExplicitUndefined = AllRequired<ExplicitUndefinedSource>;

// ---------------------------------------------------------------------------
// (5) Combined `-readonly -?`
// ---------------------------------------------------------------------------
export type ReadonlyOptionalSource = {
  readonly a?: string;
  readonly b?: number;
};
export type WritableRequired<T> = { -readonly [K in keyof T]-?: T[K] };
export type WritableRequiredResult = WritableRequired<ReadonlyOptionalSource>;

// ---------------------------------------------------------------------------
// (6) `as never` filter to drop keys whose name starts with "_"
// ---------------------------------------------------------------------------
export type FilterSource = {
  _internal: string;
  visible: number;
  _hidden: boolean;
};
export type DropPrivate<T> = {
  [K in keyof T as K extends `_${string}` ? never : K]: T[K];
};
export type DropPrivateResult = DropPrivate<FilterSource>;

// ---------------------------------------------------------------------------
// (7) Mapped type with conditional value expression
// ---------------------------------------------------------------------------
export type ValueSource = { a: string; b: number; c: "literal" };
export type StringValuesOnly<T> = {
  [K in keyof T]: T[K] extends string ? T[K] : never;
};
export type StringValuesOnlyResult = StringValuesOnly<ValueSource>;

// ---------------------------------------------------------------------------
// (8) Generic-constrained-key mapped (Pick2-style)
//
// The key union is a generic parameter constrained to `keyof T`. The mapped
// type instantiates `K = "a" | "c"` and projects only those members from `T`.
// Equivalent to TypeScript's built-in `Pick<T, K>`.
// ---------------------------------------------------------------------------
export type Pick2<T, K extends keyof T> = { [P in K]: T[P] };
export type Pick2Result = Pick2<{ a: number; b: string; c: boolean }, "a" | "c">;

// ---------------------------------------------------------------------------
// (9) Modifier idempotence — `+readonly` over an already-readonly source
//
// Applying the `+readonly` mapped form to a source whose members are already
// readonly is a no-op at the structural surface. Both members survive,
// readonly stays set, optional stays false.
// ---------------------------------------------------------------------------
export type AlreadyReadonly = { readonly a: string; readonly b: number };
export type ReadonlyOverReadonly = AllReadonly<AlreadyReadonly>;

// ---------------------------------------------------------------------------
// (10) `as` rename without filter (Capitalize-rename)
//
// Every key survives because `K extends string` holds for every string key in
// the source. The `Capitalize<K>` template literal helper rewrites each
// key to its capitalized form.
// ---------------------------------------------------------------------------
export type CapitalizeKeys<T> = {
  [K in keyof T as K extends string ? Capitalize<K> : never]: T[K];
};
export type CapitalizedResult = CapitalizeKeys<{ alpha: number; beta: string }>;
