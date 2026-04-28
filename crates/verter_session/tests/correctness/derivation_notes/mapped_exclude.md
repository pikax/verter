# TS spec §4.4 — Predefined `Exclude<T,U>` utility (distributive conditional)

`Exclude<T, U>` removes from a union every member assignable to `U`.
Defined in `lib.es5.d.ts` as:

```ts
type Exclude<T, U> = T extends U ? never : T;
```

This is a *distributive conditional* (TS spec §4.6) — the conditional
distributes over the union members of `T`. For `T = 'a' | 'b' | 'c'`
and `U = 'b'`:

1. `'a' extends 'b' ? never : 'a'` → `'a'`
2. `'b' extends 'b' ? never : 'b'` → `never`
3. `'c' extends 'b' ? never : 'c'` → `'c'`

Result: `'a' | never | 'c'` = `'a' | 'c'`.

Component-meta surface (rule-derived): one required prop `kind`
whose `type_signature` is the resulting union: `"a" | "c"`
(string-literal syntax in the canonical form).

KNOWN DEFECT (Phase 0a baseline 2026-04-28):
Verter's macro-resolution path does NOT evaluate `Exclude<>`. The
prop surfaces as `kind: /*unknown*/ semanticMiss`. Captured as a
regression baseline; future phases that implement Exclude
evaluation must re-derive the expected via the author-first
generator (see `mapped_partial.md` for the regen recipe). The
phase brief MUST carry `EXPECTS_SNAPSHOT_REGEN: <reason>`.

Tracking: see `phase-00-tier1-mismatches.md` →
"mapped_exclude".

Discriminating-test linkage (§0p.A.5):
- The fixture is the canonical "distributive Exclude" — drift would
  show either the wrong arm-set or `never` retained. Both detectable
  via the byte-equality check on `props[0].type_signature`.
