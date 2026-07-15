# TS spec §4.4 — Predefined `Required<T>` utility (mapped + `-?` modifier)

`Required<T>` strips the optional modifier from every property,
defined in `lib.es5.d.ts` as:

```ts
type Required<T> = { [P in keyof T]-?: T[P] };
```

For `Source = { a?: string; b?: number }`, `Required<Source>`
yields:

```
{ a: string; b: number }
```

Both members become required (TS `MappedModifier::Remove` on the
optional flag). Component-meta surface: two required props — `a:
string` and `b: number`, both with `required: true`.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropRequiredFlipped` — flipping `a.required` to
  false would mean the resolver did NOT strip the optional modifier.
  Detected.
