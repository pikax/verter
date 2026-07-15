// Archetype: literal-key exclusion — `Omit<Foo, "a">`.
//
// Stage 0 baseline characterisation: any edit to `Foo` invalidates
// the consumer (today's whole-export closure).
//
// Stage 6d post-change discriminator: editing `Foo.a` does NOT
// invalidate the consumer (the omitted member is not in the
// published surface). Editing `Foo.b` or `Foo.c` DOES invalidate
// (the consumer observes them via `Member(Foo, "b" | "c")`).
// Adding `Foo.d` DOES invalidate (new `MemberPresence(Foo, "d")`
// makes the consumer's surface grow under `Omit`).

export interface Foo {
  /** Member `a` — EXCLUDED from the published surface; edits MUST NOT
   * invalidate the consumer under path-precise semantics. */
  a: { id: number };
  /** Member `b` — present in the published surface. */
  b: { label: string };
  /** Member `c` — present in the published surface. */
  c: { count: number };
}

export type Props = Omit<Foo, "a">;
