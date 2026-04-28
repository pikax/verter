# Verter rule — TS-first resolution priority + userland shadowing

Citation source: `./.claude/skills/type-resolution` (TS-first
resolution priority + scope-walking rules) and CLAUDE.md
§"Macro Type Traversal Rule" (single shared cross-file type
resolver).

Verter rule (TS-first scope walk): when resolving a type identifier,
the resolver walks the lexical scope outward from the use site. A
declaration in the user's source file shadows any same-named
declaration in `lib*.d.ts`. This is the standard TypeScript scope
rule — the lib types are ambient declarations, and ambient
declarations have lower precedence than user declarations of the
same name.

Source (abbreviated):

```ts
type Pick<T, _K> = T;            // userland — wins
interface Source {
  alpha: string;
  beta: number;
  gamma: boolean;
}
defineProps<Pick<Source, 'alpha' | 'beta'>>();
```

The userland `Pick<T, _K> = T` ignores the second parameter and
yields the entire `Source` type. The resolver MUST pick the user's
declaration over `lib.es5.d.ts`'s `Pick`. Component-meta surface
(rule-derived): all three Source members — `alpha: string`,
`beta: number`, and `gamma: boolean` (all required).

KNOWN DEFECT (Phase 0a baseline 2026-04-28):
Verter's macro-resolution path dispatches to `lib.es5.d.ts`'s
`Pick` despite the in-scope userland declaration. Result: only two
props (`alpha` + `beta`) — the userland's "ignore _K, return T"
semantics is lost. The .snap.json captures the (incorrect) Verter
output as the regression baseline; future phases that fix the
userland-shadow precedence must re-derive the expected.

Tracking: see `phase-00-tier1-mismatches.md` →
"userland_shadowing_pick".

Discriminating-test linkage (§0p.A.5):
- A resolver that correctly applied userland-shadow precedence
  would surface `alpha`, `beta`, AND `gamma`. Drift from the
  current 2-prop baseline (e.g., regression to a different lib
  utility) would also be caught by byte-equality.
- The contrast with `mapped_pick_two_keys` is intentional — same
  call shape, different declaration in scope, different EXPECTED
  result; current Verter conflates the two.
