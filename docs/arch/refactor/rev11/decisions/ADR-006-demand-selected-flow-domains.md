# ADR-006 — Flow Uses Demand-Selected Abstract Domains

**Status:** Accepted

## Context

One binding-based solver is required, but one always-maximal state would make every operation pay for narrowing, completion, definite assignment, freshness, capture/effects, and coverage.

## Decision

Use one solver framework and one authoritative transfer/join implementation per closed flow domain. `FlowDemandPlan` activates the transitive prerequisite closure required by the result contract. Fixed points run only over the selected obligation frontier.

Structural authored-return collection is independent of endpoint completion. A private obligation ledger and finalizer construct complete results; a query cannot omit required domains manually.

## Consequences

Narrow queries stay narrow without introducing a second evaluator. Domains are independently testable while completeness remains global to the requested result.

## Rejected alternatives

- unconditional whole-function maximal lattice;
- independent per-query semantic evaluators;
- caller-selected domain masks without closure validation.
