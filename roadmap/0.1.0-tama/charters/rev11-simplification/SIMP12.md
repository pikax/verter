<!-- unified-charter-v2
id=SIMP12
name=Simplify component-meta contract tests
predecessors=SIMP3,E1
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:component-meta public and compat evidence
conflict_domains=public_protocol,test_maintenance
resource_class=ts-heavy
gate_profile=simplification-component-meta
review_profile=semantic-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP12.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP12 — Simplify component-meta contract tests

## Independently acceptable outcome and owner

Tests assert actual public output, types and built exports with less duplicate setup and no implementation-name scanner.

Current problem: Compat tests scan source for graph placeholders and pin an alias-bypassable _session allowlist. Other checks duplicate removed-method assertions or silently skip absent build artifacts.

Final owner: **component-meta public and compat evidence**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP3:** Test guidance assesses actual proof value without recursive guard-registration machinery or mandatory AI-authorship markers.
- **E1:** landed graph/protocol consumer boundary and current output-projector enforcement surface.

## Concrete surfaces and implementation

- `packages/component-meta/test/operator-terminal-no-graphnode-leak.spec.ts`
- `packages/component-meta/test/compat-native-call-surface-allowlist.test.ts`
- `packages/component-meta/test/__arch__/native-call-surface-walker.ts`
- `packages/component-meta/src/compat/native-projection.guard.spec.ts`
- `packages/component-meta/src/native-eval.spec.ts`
- `packages/component-meta/test/origin-optional.test-d.ts`
- `packages/component-meta/tsconfig.contract-tests.json` (scoped type-check configuration to add)

Delete graphNode source regex and native-call AST walker/allowlist and constant-pinning self-tests. Preserve actual placeholder-free display/schema and projection-to-Volar evidence. Consolidate redundant instance/prototype checks and local mocks. Replace silently skipped dist-name probes with required actual imports from fresh built exports when export proof is needed.

Bind the retained public-type assertions to a required `tsconfig.contract-tests.json` check. The current package build excludes spec files, and runtime Vitest does not check `expectTypeOf` assertions. Include the surviving projection type assertions and public optionality fixtures, adapting or extracting them after E1 as needed. Demonstrate that the selected files and assertions are checked by TypeScript; a successful ordinary build or runtime test alone is insufficient. Keep this configuration limited to the live public contracts, without reinstating retired-method tombstones or adding a type-test framework.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No public DTO/API, native ABI, wire schema, runtime implementation or unplugin changes.

## Acceptance and proportionate proof

- **SIMP12-AC1:** Compat source-name scanners and exclusive walkers are removed.
- **SIMP12-AC2:** Real public types are checked by an explicitly bound TypeScript command; display/schema and Volar projection behavior stay tested.
- **SIMP12-AC3:** Required artifact proof uses fresh exports and cannot pass by skipping missing artifacts.

Existing evidence to retain or extend: Fresh native and TS builds; native-eval, compat mapping and actual export-map imports with nonzero execution and no unexpected prerequisite skips; retained public-type fixtures selected and checked by the bound TypeScript command.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Run the following final gates in order:

1. `pnpm build:native`
2. `pnpm build:ts`
3. `pnpm --filter @verter/component-meta exec vitest run --exclude '**/checker.benchmark.spec.ts' --passWithNoTests=false`
4. `pnpm --filter @verter/component-meta exec tsc --noEmit -p tsconfig.contract-tests.json`

The runtime command explicitly includes native-eval after fresh builds and preserves the normal exclusion of the externally provisioned benchmark suite. This implementation must add the scoped type-check configuration before claiming its final gate passes.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
