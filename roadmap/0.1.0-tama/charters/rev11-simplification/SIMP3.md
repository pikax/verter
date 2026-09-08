<!-- unified-charter-v2
id=SIMP3
name=Retire recursive test-governance machinery
predecessors=SIMP1,D3C,E1,SG0
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:shared testing guidance and meaningful proof
conflict_domains=test_maintenance
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
charter=charters/rev11-simplification/SIMP3.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP3 — Retire recursive test-governance machinery

## Independently acceptable outcome and owner

Test guidance assesses actual proof value without recursive guard-registration machinery or mandatory AI-authorship markers.

Current problem: A documentation-heading registry scans named guards and has additional tests pinning its own membership. A grandfather clause retains old scanners despite the ban on adding new ones.

Final owner: **shared testing guidance and meaningful proof**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP1:** Scheduler behavior stays covered while obsolete source-shape guards and their exclusive machinery are removed.
- **D3C:** the complete atomic D3R/D3I/D3P/D3C landing, including active testing-skill and carrier changes.
- **E1:** landed graph/protocol consumer boundary and current output-projector enforcement surface.
- **SG0:** restored shipped-cfg execution and current truthful gate/testing documentation.

## Concrete surfaces and implementation

- `CLAUDE.md`
- `.claude/skills/testing/SKILL.md`
- `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`
- `crates/verter_source_policy_gate/tests/cases/handle_capable_consumer_guards.rs`

Retire CRITICAL_RULE_GUARDS, heading/name/aggregate-discovery tests, module wiring and structural_carrier_producer_guards_remain_registered. Update references in the owning shared docs and skills atomically. Remove blanket scanner grandfathering and mandatory @ai-generated test markers. Preserve substantive critical invariants and useful tests. D3C/E1 close active test/semantic policy edits; SG0 supplies current truthful gate documentation.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No semantic/provider algorithms, active PR charter edits or replacement registry/scanner framework.

## Acceptance and proportionate proof

- **SIMP3-AC1:** The heading-to-guard registry and checks pinning it are removed together.
- **SIMP3-AC2:** Testing guidance no longer requires grandfather retention or authorship-comment quotas.
- **SIMP3-AC3:** Live behavioral/type contracts and truthful gate disclosures remain intact.

Existing evidence to retain or extend: Map meaningful behavior/type/capability evidence for substantive rules affected by removed references. Existing sufficient evidence is valid; registry membership is not proof.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

node scripts/gate.mjs; node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict; pnpm docs:build.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
