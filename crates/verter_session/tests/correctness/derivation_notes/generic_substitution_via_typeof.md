# TS spec §3.6 — Generic substitution with `typeof` of a value binding

`typeof v` extracts the inferred type of a value binding. For an
object literal `const sample = { id: 'a', count: 42 }`, TS infers a
*widened* shape unless the literal is asserted (e.g., `as const`).
Without `as const`, `typeof sample.id` widens to `string`.

Source:

```ts
const sample = { id: 'a', count: 42 };
type IdShape<T> = { id: T };
defineProps<IdShape<typeof sample.id>>();
```

`typeof sample.id` = `string` (widened — see TS handbook §"Type
inference"). `IdShape<string>` substitutes T → string, yielding
`{ id: string }`.

Component-meta surface (rule-derived): one required prop `id:
string`.

KNOWN DEFECT (Phase 0a baseline 2026-04-28):
Verter's macro-resolution path does NOT perform the
typeof-to-instance substitution at this position. The prop surfaces
as `id: T` (free type parameter) rather than the substituted
`string`. Captured as a regression baseline; future phases that
implement the substitution must re-derive the expected.

Tracking: see `phase-00-tier1-mismatches.md` →
"generic_substitution_via_typeof".

Discriminating-test linkage (§0p.A.5):
- A resolver that did not substitute the type argument into
  `IdShape` would surface `id: T` (a free type parameter) rather
  than `string`. Caught by byte-equality.
- A resolver that did NOT widen the typeof would surface `id: "a"`
  (literal type), also caught.
