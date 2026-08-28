---
ruling_id: "GATE-PERFORMANCE-BLOCK"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["gate architecture", "verification infrastructure"]
source_file: "MAINTAINER-DIRECTIVE-GATE-PERFORMANCE-BLOCK.md"
summary: "The canonical Rust gate takes 30+ minutes and is to be redesigned as a dedicated gate-integrity and gate-performance block owned by the program orchestrator, NOT as licence for feature agents to delete tests. Diagnosis: the gate replays an enormous test universe three times — Surface 1 runs 24,719 process-isolated tests, Surface 2 replays every verter_session binary in shared processes, Surface 3 replays 15,578 tests under debug_assertions-off — while ~250 source-scanning tests across ~40,000 lines are compiled into the heavyweight product binary and re-run under all three. trybuild is already excluded and is NOT the bottleneck. Ordered plan: (1) telemetry only, no verdict change; (2) safe deduplication, extracting source-policy checks; (3) replace the broad unique-mode surfaces with focused suites, each gated on a seeded-defect mutation proof; (4) optimise the primary universe by batching pure fixture families and sharding CI; (5) retire grandfathered scanners as structural replacements land."
supersedes: []
superseded_by: []
contradicts: []
notes: "Every unique failure class must keep exactly one owner, one enforcement mechanism, and a proof that the mechanism detects its seeded defect. A broad surface may only be deleted AFTER its focused replacement is proven to catch the defect the broad surface existed for — a seeded process-global leak for the shared-process surface, and a seeded state mutation inside a debug_assert! argument for the shipped-cfg surface. The analysis also records real scope drift: gate.mjs still documents Surface 3 as verter_session + verter_scheduler while it now replays five packages, and every admitted target must justify a concrete cfg-sensitive failure it owns."
---

# Maintainer Directive — gate performance is a dedicated block

**Status:** RATIFIED by the maintainer, 2026-08-21. Marked ASAP.

The canonical Rust gate takes over thirty minutes. It is to be redesigned as a
dedicated **gate-integrity and gate-performance block, controlled by the program
orchestrator** — not as permission for feature agents to delete tests ad hoc.

## What the diagnosis found

The gate is not slow because of exotic tests. It is slow because it runs an
enormous universe repeatedly:

| Surface | What it runs | Cost |
|---|---|---|
| 1 | full workspace, process-isolated | 24,719 tests, 579s |
| 2 | every `verter_session` binary again, shared-process | duration and test count NOT REPORTED |
| 3 | five packages with `debug_assertions` off | 15,578 tests, 347s |

Surface 3 alone replays about 63% as many tests as Surface 1. The two timed
surfaces are already 926 seconds, and Surface 2's cost is unknown because the
runner reports only how many suite binaries passed.

Corrections to common assumptions: **`trybuild` is already excluded from every
surface and is not the bottleneck**; the Vue macro oracle costs effectively zero;
and the `18.00 GiB` line is the configured abort ceiling, not observed usage —
peak RSS is measured internally and then discarded on success.

## What has accumulated

`architecture_guards.rs` (~178 tests) and `output_projector_residual_guards.rs`
(~73 tests) total roughly 250 tests over 40,000+ lines. They walk directories,
re-read production files, search for forbidden strings, and parse Rust with
`syn` — and, being compiled into the consolidated `verter_session` binary, they
run under all three surfaces. A source scan has no shared-process failure mode
and no `debug_assertions` sensitivity, so two of those three runs cannot change
their result.

This also contradicts the repository's own rule that landed enforcement is
structural and that new name-keyed scanners must not land. The project already
knows this is not the target architecture; it has not retired the inventory.

## The rule that governs the redesign

Every required failure class keeps **one owner, one enforcement mechanism, and a
proof that the mechanism detects its seeded defect.** The strongest gate is not
the one running the most assertions repeatedly.

A broad surface may only be removed **after** its focused replacement is proven
against a seeded defect:
- shared-process: a seeded production-global leak, and a seeded failure to reset
  global state, must fail the new suite;
- shipped-cfg: a seeded state mutation inside a `debug_assert!` argument must
  fail the replacement lint or focused test.

## Ordered plan

1. **Telemetry only** — no verdict change. Report Surface 2's duration and
   executed-test count, per-phase peak RSS, per-binary cumulative time, archive
   and binary sizes, and the 50 heaviest test families.
2. **Safe deduplication** — extract the pure source-policy and freshness checks
   into one lightweight runner that walks and parses once; exclude them
   structurally from the shared-process and shipped-cfg runs; drop the
   non-blocking oversize advisory from the critical path; correct the stale
   Surface 3 documentation.
3. **Replace the broad unique-mode surfaces** with focused suites, each gated on
   its seeded-defect proof, and only then delete the blanket replays.
4. **Optimise the primary universe** — batch pure fixture families into
   table-driven tests that still report every failure, weight heavy tests into
   nextest groups, shard CI, and require the aggregator to prove the union of
   shard inventories exactly equals the canonical inventory.
5. **Retire grandfathered scanners** as structural replacements land. No new
   grandfathering.

## Target architecture

```text
required merge verdict
├── rust-primary             full behavioural/contract universe, sharded
├── rust-shared-process      focused process-global-state contracts
├── rust-shipped-cfg         release check + focused cfg-sensitive contracts
├── compiler-conformance     Vue/Svelte oracle, validity and goldens, once
├── architecture-policy      temporary residual legacy checks, once
├── js-unit                  independent
└── editor/e2e               independent
```

JS and E2E stay OUT of `gate.mjs`; the existing CI separation is correct, and the
merge verdict aggregates independent required jobs.
