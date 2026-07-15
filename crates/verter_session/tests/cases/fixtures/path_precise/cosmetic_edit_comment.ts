// Archetype: comment edit — parse_stable_hash invariant.
//
// Stage 0 baseline characterisation: today comment edits invalidate
// the consumer (content_hash changes).
//
// Stage 6d post-change discriminator: per R13 + R16
// `parse_stable_hash` is a structural hash over the post-shallow-
// analysis decl skeleton. Comments and whitespace are NOT in the
// skeleton. Editing this comment changes `content_hash` but NOT
// `parse_stable_hash`; the `MemberSemanticFactStore` keyed on
// `parse_stable_hash` is unchanged.

// This is a top-of-file standalone comment that the test rewrites.
// Editing this comment MUST NOT change parse_stable_hash.

export interface Foo {
  a: number; // inline comment after the declaration
  b: string;
}

export type Props = Foo;
