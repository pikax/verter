---
ruling_id: "CM1-RUNTIME-FORM-AXIS-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["CM1"]
source_file: "ARCHITECT-RULING-2026-08-24-CM1-RUNTIME-FORM-AXIS.md"
summary: "Held the runtime-form value `mixed runtime + type-declared` satisfiable without PropType<T>, via `X as () => T`, and ordered an uncovered acceptance cell rather than an exclusion. SUPERSEDED: the premise was falsified by the execution this ruling itself required as the gate."
supersedes: []
superseded_by:
  - ruling: "CM1-AUTHORED-ASSERTION-CAPTURE-2026-08-25"
    claim: "This ruling's conclusion that the value is satisfiable and owed as a demanded cell. Its detection finding stands and is cited by the successor; only the step from detection to publishability is overturned."
contradicts: []
notes: "Transcribed after the fact, when its successor needed a resolvable supersedes target. Recorded rather than discarded because its detection evidence remains correct and is cited by the successor: the analyzer does accept both authored spellings. What it could not establish from source inspection was that detection carries through to publication. Its closing requirement — that fresh execution, not source inspection, be the gate — is what overturned it, which is a property of a sound ruling rather than a defective one."
---

# Architect ruling — runtime-form axis (SUPERSEDED)

RULING: **UNCOVERED CELL OWED.** The proposition is rejected. `mixed runtime + type-declared` remains satisfiable without `PropType<T>`; no exclusion or charter amendment is ratified.

The acceptance axes are:

- `defineExpose` shape: simple, multiple, refs, computed refs, methods, mixed full-API.
- `defineExpose` type position: imported/local and source-local/project-aware.
- `defineProps` runtime form: shorthand, expanded, required, optional, `required: true`, defaulted, constructor array, nullable, and mixed runtime + type-declared.
- Constructor kind: `String`/`Number`/`Boolean`; module-owned/imported custom classes as negative controls; `PropType<T>` and setup-local classes explicitly excluded.
- Invocation: cold, warm, sequential, concurrent, batch.
- Surface: native and compat.
- Request view: overlay and base.
- Hard error: genuine `Present → UnraisableSource`.

Every demanded cell must preserve exact types, strict failures, invocation equality, request-view isolation, and native/compat agreement. [CM1.md:168](docs/arch/refactor/rev11/charters/CM1.md:168), [CM1.md:183](docs/arch/refactor/rev11/charters/CM1.md:183)

The tree establishes the disputed meaning and its non-`PropType<T>` realization:

- The analyzer’s own `prop_mixed_fixture` defines one runtime object containing ordinary constructor props alongside both `Object as () => typeof Card` and `Object as PropType<typeof Card>`. That is direct repository evidence for what “mixed” means. [macros_tests.rs:1963](crates/verter_semantic/src/analysis/macros_tests.rs:1963)
- Runtime extraction independently recognizes `X as () => T` and `X as new () => T` as authored type positions; it does not require the `PropType` identifier. [macros.rs:3003](crates/verter_semantic/src/analysis/macros.rs:3003)
- Replay lowers those assertions to their return type, and payload stamping places them on the same typed publication route. [macros.rs:999](crates/verter_semantic/src/analysis/macros.rs:999), [macros.rs:1168](crates/verter_semantic/src/analysis/macros.rs:1168)

The cells currently named `mixed` do not discharge this value. They are `[String, Date]` constructor arrays, explicitly testing all-or-nothing primitive folding. They discharge the constructor-array/control behavior, not a runtime object combining runtime and authored type declarations. [runtime_constructor_matrix.rs:420](crates/verter_session/tests/cases/runtime_constructor_matrix.rs:420), [runtime-constructor-matrix.test.ts:154](packages/component-meta/test/runtime-constructor-matrix.test.ts:154)

The amendment removed `PropType<T>` only from the Constructor-kind control. Because runtime form and constructor kind are separate charter axes—and function/constructor assertions remain—its exclusion does not empty the mixed runtime-form value. [amendment:57](docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT.md:57), [CM1.md:176](docs/arch/refactor/rev11/charters/CM1.md:176)

The owed cell must appear in both live matrices, using a non-`PropType<T>` construction such as:

```ts
defineProps({
  label: String,
  count: Number,
  flag: Boolean,
  item: { type: Object as () => { id: number } },
})
```

It must assert exact primitive folds for the runtime props; exact `{ id: number }`/`ObjectKeys(["id"])` publication for `item`; no `Unknown`, absence, omission, or fold capture; correct optional/default facts; and equality across every existing invocation and request-view mode on native and compat. This is executable coverage of an existing authored-payload route, not type-correctness repair, so the waiver does not prevent CM1 from satisfying it. Fresh execution remains required evidence; source inspection alone is not a green gate. [CLAUDE.md:213](CLAUDE.md:213), [CLAUDE.md:504](CLAUDE.md:504)

This does **not** expand the charter. CM1 already owns the matrix coverage. Until the cell lands and executes, `mixed runtime + type-declared` remains uncovered on native and compat; the post-program maintainer remains owner only of the two settled excluded type defects. [CM1.md:131](docs/arch/refactor/rev11/charters/CM1.md:131), [type waiver:37](docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md:37)

===VERTER-RECEIPT-BEGIN===
LANE: architect-runtime-form-axis
RESULT: FAIL
REVIEWED: bb795bf3a26fa85dcddf90a8e72b110da201ec0f
FINDINGS: 1
FINDING CM1-RUNTIME-FORM-001 | P1 | docs/arch/refactor/rev11/charters/CM1.md:176 | Proposed exclusion would remove a satisfiable but currently uncovered runtime-form value; non-PropType function/constructor assertions realize it.
===VERTER-RECEIPT-END===
