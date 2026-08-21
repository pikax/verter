---
ruling_id: "SINGLE-TEST-UNIVERSE"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["gate architecture", "verification infrastructure"]
source_file: "MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md"
summary: "Surface 2 and Surface 3 are not necessary as separate test universes. The gate keeps ONE full workspace build and ONE full nextest run. Surface 2's blanket rerun is removed and shared-process lifecycle tests are added to that single normal universe rather than to a separate surface. Surface 3 is not retained as a 15,578-test replay nor as a second whole-workspace archive; only a SMALL alternate-profile compile/runtime guard remains, and only until semantic dependence on debug_assertions and implicit overflow behavior has been structurally eliminated. Once eliminated, no-debug-assertions becomes the canonical full-test profile and Surface 3 is removed entirely."
supersedes:
  - ruling: "ONE-BUILD-ONE-RUN"
    claim: "Refines the replacement shapes. ONE-BUILD-ONE-RUN placed the shared-process contract in a dedicated `shared_process_contract` target; this directive places shared-process lifecycle tests in the NORMAL universe instead. It also states plainly that Surface 3 is not to be retained as a second whole-workspace archive. The seeded-defect deletion bar, the structural-elimination prerequisites, and orchestrator ownership all stand unchanged."
superseded_by: []
contradicts: []
notes: "Adopted by the maintainer as a decision, not a proposal. The deletion bar from ONE-BUILD-ONE-RUN remains absolute: a surface is removed only after a seeded defect proves its replacement catches what the surface existed for. Structural elimination prerequisites are unchanged — no semantic work inside debug_assert!, no semantic production code behind cfg(debug_assertions), explicit checked/wrapping/saturating arithmetic."
---

# Maintainer Directive — one test universe

**Status:** ADOPTED by the maintainer, 2026-08-21.

The decision:

1. **Remove Surface 2 as a blanket rerun.**
2. **Do not keep Surface 3** as a 15,578-test replay or as a second
   whole-workspace archive.
3. **Keep one full workspace build and one full nextest run.**
4. **Add explicit shared-process lifecycle tests to that normal universe** —
   not to a separate surface.
5. **Retain only a small alternate-profile compile/runtime guard**, and only
   until semantic dependence on `debug_assertions` and implicit overflow
   behaviour has been structurally eliminated.
6. **After those dependencies are eliminated**, make `no-debug-assertions` the
   canonical full-test profile and remove Surface 3 entirely.

## What this changes from ONE-BUILD-ONE-RUN

That directive replaced Surface 2 with a dedicated
`verter_session/tests/shared_process_contract.rs` target. This directive puts
those lifecycle tests in the **normal universe** instead. The content is the
same — create/use/drop/recreate, multiple projects in one process, cache and
registry invalidation, scheduler shutdown and restart, environment restoration,
`OnceLock` and singleton lifecycle, failure then recovery, repeated
initialization under different configurations — but it is not a separate
surface with its own archive.

It also settles Surface 3 explicitly: the second whole-workspace archive goes.

## What does NOT change

**The deletion bar is absolute.** A surface is removed only after a seeded
defect proves its replacement fails without the fix:

| Seeded defect | Required detector |
|---|---|
| process-global cache leaks between two sessions | shared-process lifecycle tests |
| runtime cannot reinitialize after shutdown | shared-process lifecycle tests |
| state transition placed inside `debug_assert!` | clippy/macro policy |
| public or cross-crate item hidden by `cfg(debug_assertions)` | shipped-cfg compile check |
| arithmetic differing when overflow checks are off | focused shipped-cfg contract |
| shipped configuration silently selects zero tests | independent expected inventory |

The structural-elimination prerequisites for step 6 are unchanged: no semantic
work inside `debug_assert!` (enforced via clippy `disallowed-macros` plus a
Verter macro taking a precomputed bool — not a name scanner), no semantic
production code behind `cfg(debug_assertions)`, and explicit
`checked_*`/`wrapping_*`/`saturating_*` where overflow is meaningful.
