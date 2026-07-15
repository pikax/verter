// Archetype: indexed-access chain — `Foo["a"]["b"]`.
//
// Stage 0 baseline characterisation: any edit to Foo (any path) marks
// the consumer dirty (today's coarse cascade).
//
// Stage 6d post-change discriminator: only edits to the EXACT path
// `Foo.a.b` invalidate the consumer. Edits to `Foo.a.c`, `Foo.b`, or
// `Foo.a.b.x.y` (deeper than the navigation terminal) do NOT
// invalidate.

export interface Foo {
  a: {
    /** The terminal hop the consumer reads; edits here MUST invalidate. */
    b: { value: number; label: string };
    /** Sibling of the terminal; edits MUST NOT invalidate. */
    c: { unrelated: string };
  };
  /** Sibling of the parent hop; edits MUST NOT invalidate. */
  b: { other: string };
}

export type Selected = Foo["a"]["b"];
