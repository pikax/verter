# TS spec §4.5 — Template literal types in mapped-key positions

Template literal types compose with mapped types via the `as` clause
(or directly in the key position). When the source union is iterated
and a template literal interpolates each member into the key, the
resulting object type has computed keys.

Source:

```ts
type PrefixedKeys<K extends string> = { [P in `prefix${K}`]: number };
defineProps<PrefixedKeys<'A' | 'B'>>();
```

For `K = 'A' | 'B'`, the mapped iteration produces:

1. `prefixA` (from `P = 'A'`)
2. `prefixB` (from `P = 'B'`)

Each key maps to `number`. Result: `{ prefixA: number; prefixB: number }`.

Component-meta surface (rule-derived): two required props —
`prefixA: number` and `prefixB: number`.

KNOWN DEFECT (Phase 0a baseline 2026-04-28):
Verter's macro-resolution path does NOT iterate the template-literal
key positions. The prop set surfaces as EMPTY (zero props). Captured
as a regression baseline; future phases that implement template-
literal-key iteration must re-derive the expected.

Tracking: see `phase-00-tier1-mismatches.md` →
"template_literal_as_key".

Discriminating-test linkage (§0p.A.5):
- A resolver that did not interpolate `${K}` would surface a
  single key `prefix${K}` (template literal preserved). Caught by
  byte-equality.
- A resolver that lost the `'B'` arm would surface only `prefixA`.
  Caught.
