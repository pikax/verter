# TS spec §4.4 — Predefined `Omit<T,K>` utility

`Omit<T, K extends string | number | symbol>` is defined in
`lib.es5.d.ts` as the complement of `Pick`:

```ts
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
```

For `Source = { alpha: string; beta: number; gamma: boolean; delta: string }`
and `K = 'alpha' | 'beta'`:

1. `keyof Source` = `'alpha' | 'beta' | 'gamma' | 'delta'`.
2. `Exclude<..., 'alpha' | 'beta'>` = `'gamma' | 'delta'`.
3. `Pick<Source, 'gamma' | 'delta'>` = `{ gamma: boolean; delta: string }`.

Component-meta surface: two required props — `gamma` (type `boolean`)
and `delta` (type `string`). The keys excluded by `K` are absent.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropMissingKey` — dropping `delta` (or `gamma`)
  would mean the resolver over-excluded. Detected.
