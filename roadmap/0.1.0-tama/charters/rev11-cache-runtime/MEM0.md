<!-- unified-charter-v2
id=MEM0
name=Aggregate semantic memory budget and workload contract
predecessors=A6
phase=rev11
train=rev11.cache-runtime
product=rev11
kind=contract
semantic_role=delivery
class=foundational
owner=rev11.cache-runtime:measured aggregate semantic memory budgets and workload authority
conflict_domains=program_authority,performance_evidence
resource_class=rust-mixed
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-cache-runtime/MEM0.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# MEM0 — Aggregate semantic memory budget and workload contract

## Independently acceptable outcome

Ratify the measured aggregate memory budget, allocation ownership inventory and hermetic lifecycle workload that physical storage and admission policy consume. This node changes plan/evidence/tooling artifacts only; it implements no retention policy and deletes no production owner. A6 supplies the accepted baseline and metric methodology.

## Binding boundaries

Follow contracts/resource-and-finalization.md, Accounting and ownership and L1. Produce catalogs/semantic-memory-budget.toml, its schema, fixed source/action fixtures and a validator. Bind exact current producer/consumer symbols for parse snapshots, graph/interner regions, fact signatures, cache candidates and public result pins. The contract must have finite measured byte limits and a truthful distinction between cache-owned, active and externally pinned bytes.

## Acceptance and evidence

- **MEM0-AC1 — sole-owner outcome:** every retained allocation class has exactly one charge owner and E4/MEM1 deletion or migration owner; shared references cannot count twice or disappear during ownership transfer.
- **MEM0-AC2 — positive contract:** normal/pressure budgets, active request limits, per-entry caps, pinned-result policy and exact controlling metric rows are ratified from recorded baseline measurements before policy changes. Missing limits fail catalog validation.
- **MEM0-AC3 — incremental equivalence:** at least 10,000 fixed workload actions cover the contract's required lifecycle classes and restore a comparable control live set every sampling tranche; an omitted class or unresolved fixture fails validation.
- **MEM0-AC4 — bounded work:** list executable build/run/negative-control commands, required local or CI lane, baseline provenance and sampling/RSS methodology. Trial the runner ingredients on current source; do not rank an incorrect historical result as equivalent work.

## Verification and consumers

Run the budget/workload schema and fixture validation plus docs-domain. E4 consumes allocation ownership, MEM1 consumes limits/admission policy, and L1/L2 consume the fixed workload and measurement rules. No new runtime capability is accepted here. Abort for unowned allocation classes or a proposed policy that would revoke live public handles.

## Review and completion

Apply the node's fresh review profile and the bound final gate; affected findings and evidence are rerun after material changes. Transition only this node's predeclared implementation row inside its own implementation patch before review. Commit message, approximate date and optional PR are locator hints only. This charter amendment leaves the node pending.
