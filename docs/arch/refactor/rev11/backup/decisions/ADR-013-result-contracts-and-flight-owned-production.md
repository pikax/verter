# ADR-013 — Result Contracts Are Separate from Execution Policy; Flights Own Producers

**Status:** Accepted  
**Decision owner:** QueryRuntime and same-key computation  
**Reopen only if:** a query family proves safe cross-snapshot in-flight sharing or a different producer model with stronger evidence.

## Context

Mixing budgets/deadlines into reusable identity fragments caches, while omitting observable approximation/exactness contracts can let a weaker result satisfy a stronger request. Binding production to the first requester makes leader cancellation abandon followers. Joining semantic work across snapshots before the producer read set is known can cause wasted waits and retry storms.

## Decision

`ResultContractId` contains every observable complete-result requirement not already owned by a separately keyed profile ID: operation/product shape, exactness/completeness, capability/unsupported/degradation policy, requested approximation mode, and required mapping/diagnostic/serialization outcome. It does not duplicate semantic/output/presentation/serialization profile values.

`ExecutionPolicy` contains waiter-local deadline, cancellation, priority, and ordinary work/time/memory budget. Budget exhaustion is partial/failure, not a weaker complete mode.

`QueryIdentity<Q>` excludes snapshot basis and contains only dimensions observable at that typed query boundary. It is used to locate bounded cached candidates, each of which carries complete read facts and is value-validated. `SemanticFlightKey<Q>` adds exact `InputBasisId` for in-flight production. Terminal presentation/serialization uses separate identities when the typed semantic result is unchanged.

Two default flight classes exist:

- content artifact flight keyed by immutable construction identity and shareable across snapshots;
- semantic query flight keyed by semantic arguments, `ResultContractId`, and exact `InputBasisId`; cross-snapshot joining is disabled by default.

The `FlightCell`, not the first waiter, owns production. Waiters register independently. The producer continues while valid waiters remain, receives bounded priority/budget aggregation, finalizes exactly once, and publishes only through the owner's sealed admission path. Every follower validates before use.

## Consequences

- leader cancellation cannot strand followers;
- ordinary budgets do not fragment reusable identity;
- incompatible result contracts never share a producer/value;
- unrelated snapshot changes do not hide a still-valid candidate;
- cross-snapshot warm reuse remains possible through value-side validation while in-flight semantic joining remains exact-basis by default.

## Rejected alternatives

- **Leader-owned promise/future:** incorrect cancellation ownership.
- **All policy in one computation key:** over-keyed and semantically ambiguous.
- **Default cross-snapshot join:** waits on work whose unknown read set may be invalid.
