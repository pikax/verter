# ADR-016 — Foundational Work Requires a Pre-Candidate Implementation Lock

**Status:** Accepted  
**Decision owner:** architecture program entry and performance governance  
**Reopen only if:** the project adopts an equally strong pre-candidate mechanism that prevents moving semantics and gates.

## Context

A strong methodology without concrete baseline, compatibility decisions, and numeric thresholds still lets implementors choose missing contracts or negotiate performance gates after a candidate exists.

## Decision

A0 captures an exact entry checkout. A1–A5 form one ordered Gate 0 lineage: command/harness fixes and the fail-closed safety retraction precede measurement and final inventories. A6 then produces one immutable Implementation Lock Record bound to:

- exact entry checkout, exact post-Gate-0 implementation baseline/tree, and repository state;
- Revision 11 package manifest digest;
- non-vacuous command/capability evidence;
- identity/profile/compatibility/protocol/dependency decisions;
- instrumentation/work baseline;
- concrete machine-readable `performance-gates.toml` with no placeholders;
- first unlocked foundational charters and review state.

Before A6, only adoption, command/capability proof, harness strengthening, wrong-complete safety retraction, measurement-only attribution, inventory, and gate/capability record work is allowed. Any Gate 0 source change invalidates and refreshes affected downstream evidence before A6. Non-safety foundational cutovers remain locked.

Gate recalibration is allowed only before the affected candidate is measured, through an exact new record digest and the same independent review class. Weakening a gate after seeing candidate results is prohibited.

## Consequences

- implementation does not invent public contracts or success criteria;
- performance decisions are reproducible and auditable;
- baseline changes trigger explicit reconciliation rather than silent drift.

## Rejected alternatives

- **Fill gates during each implementation PR:** enables outcome-driven thresholds.
- **Use prose-only “fast enough” goals:** not executable or reproducible.
