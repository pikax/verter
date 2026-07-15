# TS spec §4.4 — Mapped types and the predefined `Pick<T,K>` utility

`Pick<T, K extends keyof T>` is defined in `lib.es5.d.ts` as

```ts
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
```

For `Source = { alpha: string; beta: number; gamma: boolean; delta: string }`
and `K = 'alpha' | 'beta'`, the mapped iteration produces exactly two
properties: `alpha: string` and `beta: number`. The `optional` and
`readonly` modifiers are unchanged from the source — both source
members are required and non-readonly.

Component-meta surface: two required props named `alpha` (type
`string`) and `beta` (type `number`). No events, slots, models,
exposed, or fallthrough surface (the SFC has no `<template>` content
beyond `<div />`, no defineEmits/Slots/Model/Expose).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropExtraKey` — adding `gamma` would mean the
  resolver did NOT filter by `K`. Detected.
- `MutationKind::PropTypeChanged` — flipping `alpha`'s
  `type_signature` would mean the resolver lost `T[K]`. Detected.
