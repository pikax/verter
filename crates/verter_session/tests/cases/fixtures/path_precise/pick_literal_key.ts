// Archetype: literal-key projection — `Pick<Foo, "a">`.
//
// Stage 0 baseline characterisation: editing `Foo.b` body re-publishes
// the consumer's prop type (today's coarse whole-export closure
// invalidation). The test in
// `tests/path_precise_invalidation_baseline.rs::pick_literal_key_today_is_coarse`
// observes the consumer's `MaterializeStructureDb` slot churning under
// the edit and asserts the published prop surface re-renders.
//
// Stage 6d post-change discriminator: editing `Foo.b` does NOT
// invalidate the consumer. `MemberPresence(Foo, "a")` is unchanged
// (header invariant), `Member(Foo, "a")` is unchanged (body invariant),
// so `Pick<Foo, "a">` consumer's fact_dep_signature continues to
// validate. The test in `tests/path_precise_invalidation.rs` inverts.

export interface Foo {
  /** Member `a` — the only key the consumer selects. */
  a: { id: number; name: string };
  /** Member `b` — NOT selected; edits MUST NOT invalidate the consumer
   * under path-precise semantics. */
  b: { other: string };
  /** Member `c` — same shape as `b` for surface stability. */
  c: { extra: boolean };
}

export type Props = Pick<Foo, "a">;
