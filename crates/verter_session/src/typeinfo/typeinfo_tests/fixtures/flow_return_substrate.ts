// @ai-generated - Synthetic FlowReturn substrate characterization fixture.
//
// Every alias is driven as `ReturnType<typeof …>` (or an indexed method
// access) from the sibling `flow_return_substrate.rs` tests. The fixture
// covers the call-return and control-transparency surface (symbolic call
// returns, the `this`-call fallback, return-free loop transparency, and
// the degraded return-bearing loop / switch / try shapes), the two
// recursion contracts (base-plus-recursion and the empty recursive
// cycle), the value-environment key-exclusion discriminator, and a mixed
// relation↔function-return coinductive component.

export declare function subLog(value: number): void;

// --- Symbolic call-return parity -------------------------------------------

export function subCallee(): { ok: string } {
  return { ok: "yes" };
}

export function subCallReturn() {
  return subCallee();
}
export type SubCallReturn = ReturnType<typeof subCallReturn>;

// --- Return-free loop transparency parity -----------------------------------
// Loops and a labeled construct with NO `return` inside are effectful but
// fall-through transparent: the trailing symbolic call return is collected.

export function subCallAfterLoop() {
  for (let i = 0; i < 3; i++) subLog(i);
  let n = 0;
  while (n < 2) n += 1;
  outer: for (const x of [1, 2]) {
    if (x > 1) break outer;
  }
  return subCallee();
}
export type SubCallAfterLoop = ReturnType<typeof subCallAfterLoop>;

// --- Unsupported `this`-call fallback parity --------------------------------

export class SubThisCall {
  helper(): number {
    return 1;
  }
  run() {
    return this.helper();
  }
}
export type SubThisCallRun = ReturnType<SubThisCall["run"]>;

// --- Degraded shapes: return-bearing loop / switch / try ---------------------

export function subLoopReturn(n: number) {
  while (n > 0) {
    return n;
  }
  return 0;
}
export type SubLoopReturn = ReturnType<typeof subLoopReturn>;

export function subSwitchReturn(value: number) {
  switch (value) {
    case 1:
      return "a";
    default:
      return "b";
  }
}
export type SubSwitchReturn = ReturnType<typeof subSwitchReturn>;

export function subTryReturn() {
  try {
    return "a";
  } finally {
  }
}
export type SubTryReturn = ReturnType<typeof subTryReturn>;

// --- Recursion contracts -----------------------------------------------------

// Base-plus-recursion: the concrete `0` seed widens to `number` and admits.
export function subBaseRecursion(n: number) {
  if (n <= 0) return 0;
  return subBaseRecursion(n - 1);
}
export type SubBaseRecursion = ReturnType<typeof subBaseRecursion>;

// Empty recursive cycle: only a self-call hold, never a concrete seed.
export function subEmptyRecursion() {
  return subEmptyRecursion();
}
export type SubEmptyRecursion = ReturnType<typeof subEmptyRecursion>;

// --- Complete unannotated functions (no semantic-miss surface) ---------------

export function subCompleteUnion(flag: boolean) {
  if (flag) return subCallee();
  return 0;
}
export type SubCompleteUnion = ReturnType<typeof subCompleteUnion>;

export function subCompleteFallthrough(flag: boolean) {
  if (flag) return subCallee();
}
export type SubCompleteFallthrough = ReturnType<typeof subCompleteFallthrough>;

// --- Value-environment key exclusion discriminator ---------------------------
// Both callers demand the same callee's return under DIFFERENT local value
// environments; the callee's return identity must not see them.

export function subShared() {
  return subCallee();
}
export function subCallerA() {
  const tag = "a";
  subLog(tag.length);
  return subShared();
}
export function subCallerB() {
  const tag = 42;
  subLog(tag);
  return subShared();
}
export type SubCallerA = ReturnType<typeof subCallerA>;
export type SubCallerB = ReturnType<typeof subCallerB>;

// --- Mixed relation ↔ function-return coinductive component ------------------
// Relate(SubMixedA, SubMixedB) relates the `next` returns, which demands
// SubMixedA.next's body-derived return (the Flow leg), which resolves to
// SubMixedB and relates it back against SubMixedA — a coinductive
// assumption on the open relation.

export interface SubMixedB {
  next(): SubMixedA;
}
export declare function subMakeB(): SubMixedB;
export class SubMixedA {
  next() {
    return subMakeB();
  }
}
export type SubMixedAssign = SubMixedA extends SubMixedB ? "yes" : "no";
