// Archetype: whole-surface projection — `keyof Foo`.
//
// Stage 0 baseline characterisation: edits to any Foo body member or
// the addition of any new member invalidate the consumer (today's
// coarse cascade).
//
// Stage 6d post-change discriminator: per R28's two-fact model the
// consumer observes ONLY `MemberShape(Foo, Type)` (ordered list of
// `(name, kind)`). Editing a member body does NOT change
// `MemberShape` and so does NOT invalidate. Adding or removing a
// member or changing its kind DOES invalidate.

export interface Foo {
  a: { v: number };
  b: { v: string };
  c: { v: boolean };
}

export type Keys = keyof Foo;
