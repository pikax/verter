// @ai-generated - Synthetic CFG narrowing-invalidation fixture.
//
// Codifies TS7 "trap" cases where flow narrowing is gained then LOST (or
// surprisingly PRESERVED). These complement flow_return_catalog.ts by
// targeting the resolver's CFG model: self-assignment, opaque-call
// invalidation, closure capture, destructured discriminants, finally
// override, asserts on dotted paths, and assertNever exhaustive tails.
//
// Each exported function exposes its TS7 return type through a probe
// alias `<Name>Result = ReturnType<typeof <fn>>`.
//
// Intentional TS7 diagnostic case (NOT a fixture bug):
//   * `fi05DestructLoses` deliberately reassigns the destructured
//     discriminant `kind` to `"b"` and then compares `kind === "a"`. TS7
//     emits `TS2367` ("This comparison appears to be unintentional...")
//     because the reassignment narrows `kind` to `"b"`. The diagnostic is
//     EXACTLY what the test characterises (narrowing-loss diagnostic
//     separate from the return-type emission); the return-type tracking
//     under that diagnostic IS the contract the resolver must implement.

// ----- 1) Narrowing invalidated by self-assignment -----------------------
// After `x = 1` the narrowed `string` branch becomes `number` (the type of
// `1` after assignment-narrowing), so the return is `number`. The else
// branch returns the original `string | number` minus `string` = `number`.
// Joined: `number`.
export function fi01ReassignInvalidates(x: string | number) {
  if (typeof x === "string") {
    x = 1;
    return x;
  }
  return x;
}
export type Fi01ReassignInvalidatesResult = ReturnType<typeof fi01ReassignInvalidates>;

// ----- 2) Narrowing PRESERVED across opaque call -------------------------
// TS does NOT invalidate the local narrowing on an opaque function call.
// The if-branch return stays `string`; the else returns `number`. Joined:
// `string | number`.
export declare function fi02UnknownCall(x: unknown): void;
export function fi02PreservedAcrossCall(x: string | number) {
  if (typeof x === "string") {
    fi02UnknownCall(x);
    return x;
  }
  return x;
}
export type Fi02PreservedAcrossCallResult = ReturnType<typeof fi02PreservedAcrossCall>;

// ----- 3) Narrowing under closure capture --------------------------------
// `register` may later invoke the callback, but TS only invalidates a
// closure-captured local that is actually mutated in the *current scope's
// observed control flow*. The synchronous return point still sees
// `string`. Joined return: `string | number` (string from if-branch,
// number from else).
export function fi03CaptureInvalidates(x: string | number, register: (cb: () => void) => void) {
  if (typeof x === "string") {
    register(() => {
      x = 1;
    });
    return x;
  }
  return x;
}
export type Fi03CaptureInvalidatesResult = ReturnType<typeof fi03CaptureInvalidates>;

// ----- 4) Destructured-discriminant PRESERVES correlation ----------------
// TS propagates the discriminated-union narrowing through the `kind` local
// when it is destructured from `s` and not reassigned. `kind === "a"`
// narrows `s` to the `a` arm.
export type Fi04Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export function fi04DestructPreserves(s: Fi04Shape) {
  const { kind } = s;
  if (kind === "a") return s.a;
  return s.b;
}
export type Fi04DestructPreservesResult = ReturnType<typeof fi04DestructPreserves>;

// ----- 5) Destructured-discriminant LOSES correlation after reassignment --
// `let { kind }` followed by `kind = "b"` breaks the discriminant link.
// At `kind === "a"` we treat `s` as the *full* `Fi04Shape` union (NOT the
// `{ kind: "a"; a: string }` arm). The if-branch returns `s` itself; the
// else branch also returns `s`. Both points return `Fi04Shape`, proving
// `s` was not narrowed. We do NOT touch `s.a` because that would generate
// an unrelated property-access error; we only assert the narrowing-loss
// observable in the return type.
export function fi05DestructLoses(s: Fi04Shape) {
  let { kind } = s;
  kind = "b";
  if (kind === "a") return s;
  return s;
}
export type Fi05DestructLosesResult = ReturnType<typeof fi05DestructLoses>;

// ----- 6) `finally { return }` overrides try / catch returns -------------
// TS7: when finally has a top-level return, it always wins. Result is
// the single literal `"from-finally"`.
export function fi06FinallyOverrides() {
  try {
    return "from-try" as const;
  } catch {
    return "from-catch" as const;
  } finally {
    return "from-finally" as const;
  }
}
export type Fi06FinallyOverridesResult = ReturnType<typeof fi06FinallyOverrides>;

// ----- 7) `finally { ... }` without return preserves try/catch returns ----
// TS7: a finally that doesn't return doesn't override. The function's
// inferred return is the union `"from-try" | "from-catch"`.
export declare function fi07Cleanup(): void;
export function fi07FinallyPreserves() {
  try {
    return "from-try" as const;
  } catch {
    return "from-catch" as const;
  } finally {
    fi07Cleanup();
  }
}
export type Fi07FinallyPreservesResult = ReturnType<typeof fi07FinallyPreserves>;

// ----- 8) `asserts x is T` on dotted member path -------------------------
// The asserts predicate operates on the dotted path `c.value`. After the
// assert, `c.value` is narrowed to `NonNullable<string | undefined>` =
// `string`.
export type Fi08Container = { value: string | undefined };
export declare function fi08AssertNonNullable<T>(value: T): asserts value is NonNullable<T>;
export function fi08AssertDottedPath(c: Fi08Container) {
  fi08AssertNonNullable(c.value);
  return c.value;
}
export type Fi08AssertDottedPathResult = ReturnType<typeof fi08AssertDottedPath>;

// ----- 9) `exhaustiveCheck(value: never): never` exhaustive tail ----------
// In the default case, `shape` has narrowed to `never` after the two
// concrete kinds are handled. The default branch returns a never-typed
// call, which contributes `never` to the join. The function's inferred
// return is `string | number` (from cases "a" and "b").
export type Fi09Shape = { kind: "a"; a: string } | { kind: "b"; b: number };
export declare function fi09ExhaustiveCheck(value: never): never;
export function fi09Exhaustive(shape: Fi09Shape) {
  switch (shape.kind) {
    case "a":
      return shape.a;
    case "b":
      return shape.b;
    default:
      return fi09ExhaustiveCheck(shape);
  }
}
export type Fi09ExhaustiveResult = ReturnType<typeof fi09Exhaustive>;
