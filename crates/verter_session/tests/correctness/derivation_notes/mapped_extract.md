# TS spec §4.4 — Predefined `Extract<T,U>` utility (distributive conditional)

`Extract<T, U>` is the dual of `Exclude` — keeps only union members
of `T` assignable to `U`. Defined in `lib.es5.d.ts` as:

```ts
type Extract<T, U> = T extends U ? T : never;
```

For `T = 'a' | 'b' | 'c'` and `U = 'a' | 'b'`, the distributive
conditional reduces:

1. `'a' extends 'a' | 'b' ? 'a' : never` → `'a'`
2. `'b' extends 'a' | 'b' ? 'b' : never` → `'b'`
3. `'c' extends 'a' | 'b' ? 'c' : never` → `never`

Result: `'a' | 'b' | never` = `'a' | 'b'`.

Component-meta surface (rule-derived): one required prop `kind`
whose `type_signature` is `"a" | "b"`.

KNOWN DEFECT (Phase 0a baseline 2026-04-28):
Verter's macro-resolution path does NOT evaluate `Extract<>` (same
root cause as `mapped_exclude`). The prop surfaces as
`kind: /*unknown*/ semanticMiss`. Captured as a regression baseline.

Tracking: see `phase-00-tier1-mismatches.md` →
"mapped_extract".

Discriminating-test linkage (§0p.A.5):
- Pairs with `mapped_exclude` to cover both halves of distributive
  reduction.
