# TS spec §3.10 — Intersection of object types

When two object types are intersected, the result is an object type
whose member set is the union of both sources' members. If the same
key appears in both, the resulting member type is the intersection
of the two types (TS spec §3.10 — Intersection types).

Source:

```ts
defineProps<{ a: string } & { b: number }>();
```

Result: `{ a: string; b: number }`.

Component-meta surface: two required props — `a: string` and
`b: number`. No conflict resolution needed (disjoint key sets).

Discriminating-test linkage (§0p.A.5):
- A resolver that lost an intersection arm would surface only one
  prop. Caught by byte-equality on the prop set.
- A resolver that double-counted would still surface a 2-prop set
  (both keys are present in exactly one arm), so the byte
  comparison is the discriminator.
