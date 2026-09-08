<!-- unified-charter-v2
id=SIMP10
name=Simplify SSR property assembly
predecessors=TCM1,CCA1O4D
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:Vue SSR emitter property assembly
conflict_domains=compiler_execution,mapping_geometry
resource_class=rust-mixed
gate_profile=targeted-domain
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
charter=charters/rev11-simplification/SIMP10.md
size=L
max_production_loc=850
max_production_files=6
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP10 — Simplify SSR property assembly

## Independently acceptable outcome and owner

Minimal property key/value/order information survives until final emission, removing reparsing and narrowing property assembly state access.

Current problem: SSR renders properties to strings, reparses their keys/values to merge handlers and rebuilds/removes strings in a module with broad state responsibilities.

Final owner: **Vue SSR emitter property assembly**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **TCM1:** landed compact mapping products and current CI/mapping consumers.
- **CCA1O4D:** landed native legacy compile-profile deletion.

## Concrete surfaces and implementation

- `crates/verter_compiler/src/template/code_gen/ssr/mod.rs`

Bind merge_duplicate_event_handlers, build_attrs_string and callers. Preserve minimal property information and spread/order boundaries until one final emission; remove generated-string parsing and reconstruction/removal passes. Separate this coherent property responsibility with narrow inputs, not a general compiler IR. TCM1 and CCA1O4D provide stable mapping/native boundaries.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No packages/unplugin, pnpm-lock.yaml, Svelte runtime/guards, shared profile/registry API or CodeTransform contract changes. PR #511 remains a separate external owner.

## Acceptance and proportionate proof

- **SIMP10-AC1:** SSR handler merging no longer reparses rendered properties.
- **SIMP10-AC2:** Handler order, spreads, output and mappings remain equivalent.
- **SIMP10-AC3:** The property owner has narrower inputs and removes measured redundant work.

Existing evidence to retain or extend: SSR/code-transform cases for handler ordering, spreads, directives, helpers and fallback output/maps; equivalent property work and allocations.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused Rust SSR/compiler/map suites, equivalent-work measurements, node scripts/gate.mjs; unchanged downstream artifact contracts.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
