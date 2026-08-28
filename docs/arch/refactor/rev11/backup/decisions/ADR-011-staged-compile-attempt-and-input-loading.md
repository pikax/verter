# ADR-011 — Project-Aware Compile Uses a Resumable I/O-Free Transaction

**Status:** Accepted  
**Decision owner:** direct compiler/project TypeInfo integration  
**Reopen only if:** a future compiler mode explicitly embeds I/O and accepts a separate architecture boundary.

## Context

A compile API that may need project inputs cannot safely hide I/O or retain OXC borrows across asynchronous loading. Projection facts must be proven to belong to the exact plan that consumes them.

## Decision

Project-aware compile follows:

```text
prepare -> plan -> project -> emit
```

- prepared syntax remains owned/borrowed by one retained owner;
- the projection batch is compact, owned, and OXC-free;
- `CompilePlanToken` binds the complete normalized request to the prepared root; the plan contains narrower deterministic projection/product/terminal subplan tokens;
- `CompileFactsBatch` binds to the exact `ProjectionPlanToken`, semantic profile, dependency/input basis, projection schema, and demand digest, so terminal-only sibling plans may reuse facts only when that projection token is exactly equal;
- `NeedInputs(LoadSet)` is a first-class resumable outcome;
- external loading occurs outside compiler/semantic compute;
- no syntax borrow or unvalidated locator crosses the loading boundary;
- each attempt reports the maximal sound missing observation set discovered;
- retries use a monotonic progress ledger, bounded waves/resources, and deterministic no-progress failure;
- stale or replayed facts are rejected before emission.

## Consequences

The same semantic kernel supports local, captured-project, in-memory, and managed-snapshot modes without hidden filesystem authority.

## Rejected alternatives

- **Synchronous hidden filesystem callbacks:** break hermeticity and coherent snapshots.
- **Reparse/replan implicitly after every missing input:** can amplify work and consume mismatched facts.
