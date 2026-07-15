// Archetype: cycle handling — `type Self = { next: Pick<Self, "next"> }`.
//
// Stage 0 baseline characterisation: today's policy walker terminates
// via the layered cycle-guard (`active_refs`) with one of four
// structural sentinel shapes documented in
// `tests/fixtures/cache_baseline/cycle_safety_failure_mode.md`:
//   - `Unknown { raw: "semanticMiss…" }`
//   - `RecursiveRef { name: "Self" }`
//   - preserved `Ref { name: "Pick", type_arguments: [Ref { name: "Self" }, …] }`
//   - bare zero-arg `Ref { name: "Self" }`
// The Stage 0 characterisation pins the sentinel onto the published
// surface and exercises the 256 KiB stack worker via
// `assert_no_stack_overflow` to prove termination.
//
// Stage 6d post-change discriminator: per R27 the worklist emits a
// canonical `CycleRef(visit_index)` placeholder; visit order is
// lexicographic by `(name, symbol_space)`; the fingerprint is
// byte-identical under source-text reordering. The Stage 6d test
// inverts the sentinel acceptance to require ONLY `CycleRef`.

export type SelfPick = {
  /** Self-referential pick — the recursive cycle the cycle guard catches. */
  next: Pick<SelfPick, "next">;
  /** Sibling — non-recursive, MUST surface as `string` regardless of cycle outcome. */
  label: string;
};
