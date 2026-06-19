// Archetype: mapped type — `{ [K in keyof Foo]: T }`.
//
// Stage 0 baseline characterisation: edits to Foo (any member) and
// edits to T (the per-member projection target) both invalidate the
// consumer.
//
// Stage 6d post-change discriminator: a mapped type observes
// `MemberShape(Foo, Type)` for the key set AND the resolved body of
// `T` per member. Adding `Foo.d` adds one more mapped output member.
// Editing `Foo.a` body but NOT changing its kind does NOT change
// `MemberShape(Foo, Type)` and does NOT invalidate (path-precise
// per R28).

export interface Foo {
  a: number;
  b: string;
  c: boolean;
}

export type ReadonlyFoo = { readonly [K in keyof Foo]: Foo[K] };
