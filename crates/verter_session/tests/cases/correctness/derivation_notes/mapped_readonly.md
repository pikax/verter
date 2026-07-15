# TS spec §4.4 — Predefined `Readonly<T>` utility (mapped + `readonly` modifier)

`Readonly<T>` adds the `readonly` modifier to every property,
defined in `lib.es5.d.ts` as:

```ts
type Readonly<T> = { readonly [P in keyof T]: T[P] };
```

For `Source = { a: string; b: number }`, `Readonly<Source>` yields
the same member set with `readonly` modifiers added:

```
{ readonly a: string; readonly b: number }
```

Component-meta surface contract (Verter rule
`./.claude/skills/component-meta`): the `readonly` modifier does NOT
flow into the runtime component contract — Vue's prop registration
does not distinguish readonly from mutable props. The
`SnapshotView::PropView` therefore omits a `readonly` field by
design. The member set must still be intact: `a: string` + `b: number`,
both required.

Discriminating-test linkage (§0p.A.5): `mapped_readonly` is not the
primary fixture for any mutation row, but the snapshot is sensitive
to:
- A resolver bug that loses members when applying `Readonly` would
  be caught by the byte-equality check on the prop set.
