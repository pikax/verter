# ADR-020 — Constitutional Invariants Are Separated from Evidence-Selected Tactics

**Status:** Accepted  
**Decision owner:** architecture confidence and amendment.  
**Reopen only if:** an alternative classification provides stronger falsifiability without allowing local weakening of product invariants.

## Context

“Best possible” is not an evidence claim that can be established before implementation. Some decisions define the product architecture; others are repository facts or tactics that should change when measurements disprove them. Conflating them creates dogma or uncontrolled redesign.

## Decision

Adopt `contracts/architecture-falsification.md`:

- classify decisions as constitutional invariants, repository/product facts, evidence-selected tactics, or deferred research choices;
- require an A6 premise ledger with falsification triggers and affected blocks;
- allow tactical changes only within locked architecture and gates;
- require ADR/architecture amendment for constitutional changes;
- stop and refresh/rescope when source or measurement falsifies a premise.

## Consequences

Revision 11 is a falsifiable architecture authority rather than a claim of global optimality. Evidence can improve the implementation without reopening core ownership casually.

## Rejected alternatives

- freeze every implementation sketch as architecture;
- let implementors reinterpret core invariants as performance tactics;
- declare optimality without a disproof mechanism.
