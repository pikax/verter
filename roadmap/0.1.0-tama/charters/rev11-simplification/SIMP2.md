<!-- unified-charter-v2
id=SIMP2
name=Retire playground engine tombstones
predecessors=A6
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:playground capability and URL restoration
conflict_domains=test_maintenance
resource_class=ts-heavy
gate_profile=simplification-playground
review_profile=simple-1
implementation_effort_min=low
implementation_effort_default=medium
review_effort_min=low
review_effort_default=low
verification_effort_min=low
verification_effort_default=medium
confirmation_effort_min=low
confirmation_effort_default=low
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP2.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP2 — Retire playground engine tombstones

## Independently acceptable outcome and owner

Browser capabilities and legacy URL restoration are tested directly without engine-name or source-layout tombstones.

Current problem: The playground recursively scans itself for retired engine words and walks again to assert an arbitrary file count. Split-up banned strings and filename checks preserve history without proving browser behavior.

Final owner: **playground capability and URL restoration**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **A6:** ratified current architecture and proportionate evidence baseline, already implemented.

## Concrete surfaces and implementation

- `packages/playground/src/editor/wasmNoLiveTsgo.spec.ts`
- `packages/playground/src/editor/wasmTsgoFailClosed.spec.ts`

Delete wasmNoLiveTsgo.spec.ts. Retain capabilityForWasm and persisted-selection deserialization in wasmTsgoFailClosed.spec.ts; remove raw TypeCheckerMode declaration matching and the retired-module glob. Simplify token splitting used solely to evade self-scanning. Remove exclusive imports and fixture setup.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No production playground, importMap, compiler mapping fixture, native/WASM API, unplugin or lockfile changes.

## Acceptance and proportionate proof

- **SIMP2-AC1:** Retired-engine source and filename scanners are removed.
- **SIMP2-AC2:** Browser capabilities and legacy URL restoration remain covered.
- **SIMP2-AC3:** Test diagnostics describe observable behavior without self-scanning workarounds.

Existing evidence to retain or extend: capabilityForWasm version boundaries and deserializeFromHash for a legacy _typeChecker selection through callable production entry points.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

`pnpm --filter @verter/playground exec vitest run src/editor/wasmTsgoFailClosed.spec.ts --passWithNoTests=false`, with normal dependencies installed. This selects the retained editor capability/URL suite without adding the separately provisioned WASM compiler lane.

Apply simple-1 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
