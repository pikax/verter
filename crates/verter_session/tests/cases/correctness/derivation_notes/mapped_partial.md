# TS spec §4.4 — Predefined `Partial<T>` utility (mapped + `?` modifier)

`Partial<T>` makes every property optional, defined in `lib.es5.d.ts`
as:

```ts
type Partial<T> = { [P in keyof T]?: T[P] };
```

The `?` after the mapped key adds the optional modifier (TS
`MappedModifier::Add`). For `Source = { a: string; b: number }`,
`Partial<Source>` yields:

```
{ a?: string; b?: number }
```

Component-meta surface: two optional props — `a: string` and
`b: number`, both with `required: false`.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropRequiredFlipped` — rule on this fixture is the
  inverse: required must NOT be true. The complementary case is
  exercised by `mapped_required` below.
