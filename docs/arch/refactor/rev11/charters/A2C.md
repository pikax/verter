# A2C — Completion topology and G10 safety verdict

**Class:** Foundational safety; early structural slice of D6’s sole flow-graph authority.  
**Predecessors:** A2.

## Objective

Extend the canonical function skeleton with the minimum content-free control topology required by the sole `FunctionFlowGraph`, then derive abrupt-completion coverage during the existing demanded graph build. Supply A3 with a typed `FlowGap::AbruptCompletion` when the current producer’s endpoint contribution contradicts the graph verdict or the verdict is unknown.

## In scope

- Canonical control-construct identities and parent/group relationships for labels, switch, loops, try, catch, and finally.
- Source-ordered content-free completion events for return, throw, labeled/unlabeled break, and labeled/unlabeled continue.
- Direct graph edges for normal continuation, return, throw-to-catch, break/continue destination, switch exit, and finally preservation/override.
- Structural authored-return membership.
- One exact-or-typed-unknown root completion-coverage verdict stored on `FunctionFlowGraph`, not `FunctionBodySkeleton`.
- A typed `FlowGap::AbruptCompletion` emitted by the flow producer when its endpoint-undefined claim disagrees with that verdict or coverage is unknown.
- Exact G10, X05, X68, X80, X88, switch/catch sibling, malformed-target, deep-target, and non-interference evidence.

## Out of scope

- Value typing, capture/effect transfer, freshness, or escape; D5 remains owner.
- Loop fixed points, slot-state transfer, narrowing, or final flow joins; later D6 work remains owner.
- Proof-carrying complete-result construction and cache-admission closure; D8 remains owner.
- AST retention, query-time AST rewalk, a completion memo, a second syntax evaluator, target-indexed completion sets, or a fixed target-count ceiling.

## Construction contract

Skeleton construction performs one syntax walk and records only canonical structural topology/events. It does not compute or retain `EndpointUndefinedFact`, `CompletionSet`, statement completion sets, suffix completion sets, or active-target bitsets.

`build_function_flow_graph(&FunctionBodySkeleton)` is the sole completion reducer. It resolves completion events to dense control identities, emits completion edges on the existing graph, and computes `CompletionCoverage`. No other production component interprets label, switch, loop, try/catch/finally, break, continue, throw, or return composition.

## A3 contract

A3 consumes only the producer’s typed degradation:

```rust
match flow_result.degradation() {
    Some(FlowGap::AbruptCompletion { .. }) => {
        // Partial/FlowGap/NoValue; suppress warm admission.
    }
    _ => {}
}
```

A3 must not read statement syntax, skeleton regions, completion events, graph edges, or an endpoint accessor.

## Required performance evidence

- Representative-corpus aggregate skeleton-index construction passes the frozen 3%/noise gate.
- Public cold flow requests pass the frozen relative gate and a predeclared absolute SLO.
- Work is linear in indexed control constructs, completion events, and emitted completion edges.
- No fixed target-capacity discontinuity; 64 and 65 live targets are ordinary exact inputs.
- Retained bytes are attributed solely to canonical D6-required topology; no A3-only retained payload exists.
- No completion-owned allocation occurs for functions containing no completion-relevant control/event beyond data already required by the skeleton.

## Abort/rescope

Stop if exact discrimination requires value typing, D5 effects, loop fixed points, D8 proof minting, AST retention/rewalk, a second graph, or a second completion classifier.
