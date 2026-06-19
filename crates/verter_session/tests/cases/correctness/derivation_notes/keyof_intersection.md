# TS spec §4.5 — `keyof` over an intersection of object types

For an intersection `A & B`, the keys are the union of A's keys and
B's keys: `keyof (A & B)` = `keyof A | keyof B`. This is documented
behaviour in TS handbook §"Mapped types" (cross-references §4.5
indexed access).

Source:

```ts
interface A { foo: string; bar: number; }
interface B { baz: boolean; }
```

`keyof (A & B)` = `'foo' | 'bar' | 'baz'`. TS does not specify an
ordering for the resulting union — Verter's canonical form
preserves the source declaration order (A first, then B).

Component-meta surface: one required prop `key` whose
`type_signature` is the union literal `"foo" | "bar" | "baz"`.

Discriminating-test linkage (§0p.A.5):
- A resolver bug that resolved `keyof` over only one intersection
  arm would surface as a smaller union. Caught by byte-equality.
- Verter rule: keyof and intersection must compose without losing
  arms (component-meta semantic invariant).
