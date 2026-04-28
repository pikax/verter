# Phase 0a — Tier 1 known mismatches (Verter resolver defects)

This file is the deferred-defect register produced by the Phase 0a
worker per §0p.A.4 case-2. It lists each Class A fixture whose
hand-derived TS-spec / Verter-rule expected value DIFFERS from
Verter's current `ComponentMetaAnalysis` output, classifies the
defect, and links to the rule citation that the resolver violates.

The .snap.json files for these fixtures capture **Verter's current
behaviour** (so the gate locks in non-drift on the regression),
NOT the rule-correct value. Each case is a known defect that future
phases must close. When a phase fixes one of these defects:

1. The phase brief MUST carry `EXPECTS_SNAPSHOT_REGEN: <reason>`
   (§0.6.4) — the .snap.json drift is intended.
2. The worker re-derives `expected.rs::<id>()` to the rule-correct
   value (per the `EXPECTED OUTPUT` column below), removes the
   `KNOWN DEFECT` annotation from `expected.rs` and the derivation
   note, runs `--ignored generate_class_a_snapshots_from_expected`,
   and removes the row from this file.

Each mismatch was verified by reading TS spec §X.Y or
`.claude/skills/...` directly — NOT by running Volar or
vue-component-meta (Tier 1 authorship rule, §0p.A.0).

---

## 1. `mapped_exclude` — Exclude<T,U> not evaluated

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.4 (`Exclude<T,U> = T extends U ? never : T`)         |
| EXPECTED       | `kind: "a" \| "c"` (distributive conditional, `'b'` filtered)  |
| ACTUAL         | `kind: /*unknown*/ semanticMiss`                               |
| ROOT CAUSE     | `Exclude<>` is not evaluated through Verter's macro path. The  |
|                | analyzer surfaces the unresolved utility as `semanticMiss`.    |
| OWNER (later)  | Type-resolver / mapped-type evaluator, likely Phase 5 (engine  |
|                | retirement) or a dedicated utility-evaluation phase.            |

## 2. `mapped_extract` — Extract<T,U> not evaluated

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.4 (`Extract<T,U> = T extends U ? T : never`)         |
| EXPECTED       | `kind: "a" \| "b"`                                             |
| ACTUAL         | `kind: /*unknown*/ semanticMiss`                               |
| ROOT CAUSE     | Same as `mapped_exclude` — the distributive-conditional        |
|                | utility is not evaluated.                                       |
| OWNER (later)  | Same as `mapped_exclude`.                                       |

## 3. `template_literal_as_key` — template-literal key iteration loses props

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.5 (template literal types in mapped key positions)   |
| EXPECTED       | `props = [prefixA: number, prefixB: number]`                   |
| ACTUAL         | `props = []` (empty)                                            |
| ROOT CAUSE     | Verter's mapped-type evaluator does not interpolate the         |
|                | template literal across the source union, dropping all keys.    |
| OWNER (later)  | Mapped-type evaluator + template-literal lowering. Touches      |
|                | `verter_semantic::analysis::type_expand` plus the mapped/typed-  |
|                | key handling in the macro resolver.                            |

## 4. `generic_substitution_via_typeof` — typeof substitution skipped

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §3.6 (generic substitution); CLAUDE.md "generic         |
|                | substitutions are part of semantic meaning".                    |
| EXPECTED       | `id: string` (after substituting `T → typeof sample.id`,        |
|                | which widens to `string`).                                      |
| ACTUAL         | `id: T` (free type parameter; no substitution performed).       |
| ROOT CAUSE     | Verter's resolver does not instantiate `IdShape<T>` with the    |
|                | `typeof sample.id` argument; T remains abstract.                |
| OWNER (later)  | Type-argument substitution path in macro resolver. Likely       |
|                | overlaps with the generic-instantiation work flagged in         |
|                | Phase 5 / Phase 7 of the cutover plan.                          |

## 5. `userland_shadowing_pick` — TS-first / userland shadow not honoured

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | Verter rule `./.claude/skills/type-resolution` ("TS-first       |
|                | resolution priority"); CLAUDE.md §"Macro Type Traversal Rule"   |
|                | (single shared cross-file type resolver).                       |
| EXPECTED       | `props = [alpha, beta, gamma]` — the userland `Pick<T,_K> = T`  |
|                | shadows lib's `Pick`, returning the entire `Source` interface.  |
| ACTUAL         | `props = [alpha, beta]` — Verter dispatches to lib's `Pick`,    |
|                | filtering by the second type argument.                         |
| ROOT CAUSE     | The macro resolver does not perform an outward lexical-scope    |
|                | walk before falling back to ambient lib declarations. Userland  |
|                | type aliases of common utility names are silently overridden.   |
| OWNER (later)  | Resolver scope-walk policy. Likely Phase 5 / engine-retirement  |
|                | scope.                                                          |

---

## Summary

5 known defects committed as Phase 0a regression baselines. None
are blockers per the Phase 0 brief: the .snap.json files lock in
**non-drift** of Verter's current (incorrect) behaviour, ensuring
later refactors do not silently change the output further. Each
defect has a derivation note citing the violated rule and a
cross-reference to this file.

The Class A fixture set still satisfies §0p.A.0's "no self-confirming
snapshot" rule because every defect carries:
- a derivation note with TS-spec / Verter-rule citation explaining
  what SHOULD be there;
- a `KNOWN DEFECT` annotation in `expected.rs` with the same rule
  reference;
- an entry in this file with classification + owner suggestion.

Future phases that close these defects must follow the regen
recipe documented at the top of this file.
