// A NON-root imported module (not a synthetic-tsconfig `files` entry — it enters
// the program only transitively via NonRootImport.vue). It carries ONE real,
// self-contained type error so the WHOLE-PROGRAM typecheck surfaces it. The old
// per-root loop queried only the generated carriers, so this diagnostic was
// dropped; the whole-program call surfaces it (parity with `tsgo --project`).

// TS2322 — string literal is not assignable to a `number`-typed constant.
export const brokenValue: number = "definitely not a number";

// A clean symbol NonRootImport.vue imports and uses (so the module is pulled
// into the program and `noUnusedLocals` on the consumer side is satisfied).
export const answer: number = 42;
