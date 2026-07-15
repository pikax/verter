// @ai-generated - typeof-narrowing scenarios.
//
// Each fn returns the value(s) reachable inside / outside a typeof guard,
// so ReturnType<typeof fn> encodes the joined narrowing emission.
// All emissions verified out-of-band against tsgo 7.0.0-dev.20260523.1
// via IsExactly probes.
//
// Documented TS7 quirks (NOT fixture bugs):
//   * Nt04: `typeof x === "object"` does NOT narrow out `null`. The if-branch
//     would keep `null` if it were in the original union; the joined return
//     still equals the original union when both arms return `x`.
//   * Nt10: For an unbounded generic `T`, `typeof x === "string"` narrows
//     `x` to `T & string` in the if-branch but does NOT narrow the else.
//     The joined ReturnType for `nt10StringOnGeneric<string | number>` is
//     `string | number` because both branches return `x` and the resulting
//     `(T & string) | T` collapses to `T` then to the instantiated union.
//   * Nt14: `typeof x === tag` where `tag` has a literal type `"string"`
//     does NOT narrow (TS only narrows `typeof` against a string-literal
//     RHS in the source — not against a variable, even if its declared
//     type is a literal). Both branches see the original union.
//   * Nt15: For `typeof x === "string" && x.length > 0`, the else branch
//     remains the FULL original union (`string | number`) because TS
//     cannot represent "string with length === 0" as a distinct arm.

// ----- 1) typeof === "string" on string | number ------------------------
// if-branch: string. else: number. Joined: string | number.
export function nt01StringOnUnion(x: string | number) {
  if (typeof x === "string") return x;
  return x;
}
export type Nt01StringOnUnionResult = ReturnType<typeof nt01StringOnUnion>;

// ----- 2) typeof === "number" on string | number | boolean --------------
// if-branch: number. else: string | boolean. Joined: string | number | boolean.
export function nt02NumberOnTriple(x: string | number | boolean) {
  if (typeof x === "number") return x;
  return x;
}
export type Nt02NumberOnTripleResult = ReturnType<typeof nt02NumberOnTriple>;

// ----- 3) typeof === "boolean" on string | boolean ----------------------
// if-branch: boolean. else: string. Joined: string | boolean.
export function nt03BooleanOnUnion(x: string | boolean) {
  if (typeof x === "boolean") return x;
  return x;
}
export type Nt03BooleanOnUnionResult = ReturnType<typeof nt03BooleanOnUnion>;

// ----- 4) typeof === "object" on Record<string, unknown> | string -------
// if-branch: Record<string, unknown> (null NOT introduced because not in original
// union). else: string. Joined: Record<string, unknown> | string.
export function nt04ObjectOnUnion(x: Record<string, unknown> | string) {
  if (typeof x === "object") return x;
  return x;
}
export type Nt04ObjectOnUnionResult = ReturnType<typeof nt04ObjectOnUnion>;

// ----- 5) typeof === "function" on (() => void) | string ----------------
// if-branch: () => void. else: string. Joined: (() => void) | string.
export function nt05FunctionOnUnion(x: (() => void) | string) {
  if (typeof x === "function") return x;
  return x;
}
export type Nt05FunctionOnUnionResult = ReturnType<typeof nt05FunctionOnUnion>;

// ----- 6) typeof === "undefined" on string | undefined ------------------
// if-branch: undefined. else: string. Joined: string | undefined.
export function nt06UndefinedOnUnion(x: string | undefined) {
  if (typeof x === "undefined") return x;
  return x;
}
export type Nt06UndefinedOnUnionResult = ReturnType<typeof nt06UndefinedOnUnion>;

// ----- 7) typeof === "bigint" on bigint | string ------------------------
// if-branch: bigint. else: string. Joined: bigint | string.
export function nt07BigintOnUnion(x: bigint | string) {
  if (typeof x === "bigint") return x;
  return x;
}
export type Nt07BigintOnUnionResult = ReturnType<typeof nt07BigintOnUnion>;

// ----- 8) typeof === "symbol" on symbol | string ------------------------
// if-branch: symbol. else: string. Joined: symbol | string.
export function nt08SymbolOnUnion(x: symbol | string) {
  if (typeof x === "symbol") return x;
  return x;
}
export type Nt08SymbolOnUnionResult = ReturnType<typeof nt08SymbolOnUnion>;

// ----- 9) typeof === "string" on unknown --------------------------------
// if-branch: string. else: unknown (no narrowing of else). Joined: unknown.
export function nt09StringOnUnknown(x: unknown) {
  if (typeof x === "string") return x;
  return x;
}
export type Nt09StringOnUnknownResult = ReturnType<typeof nt09StringOnUnknown>;

// ----- 10) typeof === "string" on generic T -----------------------------
// if-branch: T & string. else: T. For ReturnType<typeof nt10StringOnGeneric>
// the parameter T is not specified so resolves to unknown.
// The collapsed join equals unknown.
export function nt10StringOnGeneric<T>(x: T) {
  if (typeof x === "string") return x;
  return x;
}
export type Nt10StringOnGenericResult = ReturnType<typeof nt10StringOnGeneric>;

// ----- 11) typeof !== "string" on string | number -----------------------
// if-branch (negated): number. else: string. Joined: string | number.
export function nt11NegatedOnUnion(x: string | number) {
  if (typeof x !== "string") return x;
  return x;
}
export type Nt11NegatedOnUnionResult = ReturnType<typeof nt11NegatedOnUnion>;

// ----- 12) switch typeof exhaustive -------------------------------------
// case "string" -> string; case "number" -> number; case "boolean" -> boolean.
// default contributes `never` (absorbed into the join). Joined: string | number | boolean.
export function nt12SwitchTypeof(x: string | number | boolean) {
  switch (typeof x) {
    case "string":
      return x;
    case "number":
      return x;
    case "boolean":
      return x;
    default: {
      const _exhaustive: never = x;
      return _exhaustive;
    }
  }
}
export type Nt12SwitchTypeofResult = ReturnType<typeof nt12SwitchTypeof>;

// ----- 13) Negated guard with early return ------------------------------
// `if (typeof x !== "string") return x;` -> after the if, x narrowed to string
// (only path past the if is when typeof === "string"). The pre-return inside
// the if returns `number` (from the negated branch). Joined: string | number.
export function nt13NegatedEarlyReturn(x: string | number) {
  if (typeof x !== "string") return x;
  return x;
}
export type Nt13NegatedEarlyReturnResult = ReturnType<typeof nt13NegatedEarlyReturn>;

// ----- 14) typeof against a literal-type variable -----------------------
// TS does NOT narrow `typeof x === tag` even when `tag: "string"` is a
// literal-typed variable. Both branches see the FULL original union.
// Joined: string | number.
export function nt14CompareLiteralVar(x: string | number) {
  const tag: "string" = "string";
  if (typeof x === tag) return x;
  return x;
}
export type Nt14CompareLiteralVarResult = ReturnType<typeof nt14CompareLiteralVar>;

// ----- 15) Compound typeof + property guard -----------------------------
// `typeof x === "string" && x.length > 0` -> if-branch: string (length probe
// requires string). else: string | number (TS cannot represent the
// "string with length===0" arm distinctly). Joined: string | number.
export function nt15CompoundAnd(x: string | number) {
  if (typeof x === "string" && x.length > 0) return x;
  return x;
}
export type Nt15CompoundAndResult = ReturnType<typeof nt15CompoundAnd>;
