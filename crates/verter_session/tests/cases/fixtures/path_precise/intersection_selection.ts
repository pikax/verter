// Archetype: intersection selection — `(Foo & Bar)["x"]`.
//
// Stage 0 baseline characterisation: any edit to either Foo or Bar
// invalidates the consumer (today's whole-export coverage of both
// arms).
//
// Stage 6d post-change discriminator: per R14 "non-contributing
// intersection arms are ignored (not rewritten to `never`)". The
// consumer reads `"x"` from the intersection; ONLY arms that
// contribute `"x"` are observed. Edits to non-contributing fields
// of contributing arms (e.g. `Foo.y`, `Bar.y`) do NOT invalidate.
// Edits to non-contributing arms entirely (if added later) do NOT
// invalidate.

export interface Foo {
  /** Contributes `x` — observed by consumer. */
  x: { fooX: number };
  /** Non-selected; edits MUST NOT invalidate consumer. */
  y: { fooY: string };
}

export interface Bar {
  /** Contributes `x` — observed by consumer. */
  x: { barX: boolean };
  /** Non-selected; edits MUST NOT invalidate consumer. */
  y: { barY: number };
}

export type Selected = (Foo & Bar)["x"];
