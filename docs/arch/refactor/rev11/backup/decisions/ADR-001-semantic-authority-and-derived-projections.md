# ADR-001 — One Semantic Authority, Justified Derived Projections

**Status:** Accepted  
**Decision owner:** TypeInfo/effective-flow architecture

## Context

`FunctionFlowGraph` must be the one structural flow authority, but efficient solving may need dominators, loop forests, SCCs, def-use overlays, capture summaries, reverse adjacency, or compact execution schedules. Forbidding every graph-derived structure would either bloat the canonical graph or repeat topology work. Allowing independent query-specific CFGs recreates the current dual-authority defect.

## Decision

`FunctionFlowGraph` is the canonical structural authority. A derived structure is allowed only when it:

- is keyed by one exact graph/body identity and any interpretation-affecting semantic profile;
- is deterministic and fully reproducible from the graph and accepted kernel rules;
- cannot add facts, define transfer/join semantics, mark coverage complete, or publish a semantic result;
- is request-local by default and independently weighted/evicted if retention is proven valuable;
- can be replaced without changing observable semantics.

## Consequences

The solver may use efficient layouts without creating a second semantic or control authority. Review checks authority and construction rights, not superficial data-structure count.

## Rejected alternatives

- exactly one physical graph-like object;
- independent syntax-shaped/query-specific CFGs;
- projections that own relation, completion, capture, or coverage decisions.
