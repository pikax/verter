// Archetype: JSDoc edit — semantic-lane invariant, display-lane sensitive.
//
// Stage 0 baseline characterisation: today edits to JSDoc invalidate
// the consumer's published surface (the cache observes content_hash,
// not parse_stable_hash).
//
// Stage 6d post-change discriminator: per R13 facts split into
// `semantic_hash` and `display_hash`. `MemberSemanticFactStore` keys
// on `parse_stable_hash` — invariant under JSDoc edits.
// `MemberDisplayFactStore` keys on `content_hash` — recomputes display
// facts but recomputed values may equal originals when JSDoc has
// changed only formatting.

export interface Foo {
  /**
   * Member `a`'s JSDoc. The CONTENT of this docstring is what gets
   * edited in the discriminator test — the structural surface
   * (`a: number`) is unchanged.
   */
  a: number;
  /**
   * Member `b`'s JSDoc.
   */
  b: string;
}

export type Props = Foo;
