// Archetype: recursive normalised type args — `Container<Wrapper<Inner>>`.
//
// Stage 0 baseline characterisation: edits to Inner / Wrapper /
// Container all invalidate the consumer's published surface
// (today's coarse cascade).
//
// Stage 6d post-change discriminator: per "normalized_type_args"
// section, type-arg normalisation is recursive structural. Editing
// `Inner.x` body changes `Member(Inner, "x")`; the
// `normalized_type_args` for `Wrapper<Inner>` recursively recomputes
// only when Inner's TypeExpr changes structurally. Editing
// `Inner.unrelated_decl` (a top-level export sibling, not part of
// Inner's body) does NOT invalidate.

export interface Inner {
  /** Observed via Wrapper<Inner>.value.value path. */
  x: number;
}

export interface Wrapper<T> {
  /** The single hop that exposes T to the consumer. */
  value: { value: T };
  /** Sibling that does NOT touch T; edits MUST NOT invalidate Inner-bound consumers. */
  metadata: { tag: string };
}

export interface Container<U> {
  /** Single hop exposing U. */
  payload: U;
}

export type Materialised = Container<Wrapper<Inner>>;
