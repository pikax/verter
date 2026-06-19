# TS spec §4.4 — Predefined `Record<K,V>` utility (mapped over key union)

`Record<K extends string | number | symbol, V>` is defined in
`lib.es5.d.ts` as:

```ts
type Record<K extends keyof any, V> = { [P in K]: V };
```

For `K = 'x' | 'y'` and `V = number`, `Record<K, V>` enumerates the
union and assigns `V` to each key:

```
{ x: number; y: number }
```

Component-meta surface: two required props — `x: number` and
`y: number`. Both required because the mapped type does not add the
optional modifier (default `MappedModifier::None`).

Discriminating-test linkage (§0p.A.5):
- The fixture is paired with `mapped_pick_two_keys` for keyset
  filtering coverage. Drift here would surface a different prop
  set — caught by the byte-equality check.
