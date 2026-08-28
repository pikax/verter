# ADR-017 — Stacked Review Must Preserve Block Atomicity and Exact-Candidate Proof

**Status:** Accepted  
**Decision owner:** PR transport, review convergence, and landing.  
**Reopen only if:** repository hosting or review mechanics provide a stronger equivalent that preserves exact candidate identity, independently releasable layers, and atomic cutovers.

## Context

Small dependent PRs improve reviewability, especially with agent-generated work, but lower-layer changes cascade new descendant SHAs and trees. Some architecture changes are independently landable; others are review-splittable but must reach trunk atomically. Treating all stacks alike conflicts with exact-candidate approval and one-production-path cutovers.

## Decision

Adopt `contracts/stacked-prs.md`:

- the program DAG remains semantic predecessor authority and the stack remains transport;
- one declared bounded stack window covers one short connected path or one atomic group; true DAG siblings use separate windows;
- mergeable layers map to independently releasable accepted blocks or explicitly accepted subblocks;
- private review-only layers have unique layer IDs, may repeat one atomic acceptance block or represent an explicit private-checkpoint predecessor, target a private integration branch, and never merge independently;
- lower-layer changes trigger restack, cumulative-tree CI, candidate invalidation, and impact-bounded reattestation; reviewed candidate identity and accepted landing identity remain separate; SHA and full tree may differ after a base advance, but a validated proof must establish exact canonical candidate-delta and generated-output equivalence on recorded bases;
- `LANDABLE` windows land bottom-up one layer at a time and issue successor snapshots; `ATOMIC_REVIEW` windows land only the final candidate;
- sibling DAG branches normally use separate stacks;
- no program-wide stack and no independently merged `D1` or other private atomic foundation.

## Consequences

Stacks are encouraged when they reduce review burden without weakening block acceptance. Rebase churn, generated changes, and exact candidate changes are visible and proven rather than silently inherited.

## Rejected alternatives

- one PR for the entire program;
- one continuously rebased fifty-block stack;
- approvals surviving changed SHAs or trees by convention;
- merging private atomic-cutover layers independently.
