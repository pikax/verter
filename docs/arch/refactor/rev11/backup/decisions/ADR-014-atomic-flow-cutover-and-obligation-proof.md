# ADR-014 — Flow Replaces the Legacy Evaluator in One Atomic Public Cutover

**Status:** Accepted  
**Decision owner:** flow migration and cache admission  
**Reopen only if:** a release obligation explicitly requires two semantics paths, which would require a new architecture decision and bounded compatibility window.

## Context

Building graph/domain features across many accepted merges while the old syntax-shaped evaluator remains selectable creates two production authorities. Deleting the old path only after full parity pressures implementors to copy the legacy model into the replacement.

## Decision

1. Build the minimum new graph/domain/obligation/coverage foundation behind a private non-production test boundary on the bounded cutover branch; it may be reviewed as a checkpoint but never merged or released independently.
2. In one public cutover, route all effective-flow operations to the new solver and delete the old evaluator and its state/caches/tasks/flags.
3. Unsupported mechanisms return typed non-admissible gaps; temporary reduction from guessed success to honest partial is allowed only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.
4. Later semantic blocks only expand the sole solver.
5. A private obligation ledger and finalizer make complete-result construction impossible unless all required closed-domain obligations are discharged.

No runtime flag, compatibility shim, shadow evaluator, or compare-in-production path survives the accepted cutover.

## Consequences

- one production semantic authority at all times after `D2`;
- completeness proof is architectural rather than a convention;
- parity can expand without preserving guessed legacy behavior.

## Rejected alternatives

- **Long-running dual evaluators:** divergent semantics and cache risk.
- **Wait for full parity before deletion:** encourages porting the second authority intact.
