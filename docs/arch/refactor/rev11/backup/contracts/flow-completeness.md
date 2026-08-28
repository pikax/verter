# Flow Domain Closure, Obligation Ledger, and Completeness Contract

**Status:** Normative effective-flow solver and warm-admission contract.

# 1. Sole structural authority

`FunctionProgramIndex -> FunctionBodySkeleton -> FunctionFlowGraph` is the sole structural flow authority. Deterministic derived projections such as dominators, loop forests, SCCs, def-use indexes, capture summaries, and execution schedules may accelerate the graph but cannot introduce semantic facts.

# 2. Closed operation/domain registry

Every flow-bearing operation declares a static closed registry:

```text
operation/result contract
-> required product domains
-> required graph edge/fact families
-> expansion rules
-> fixed-point requirements
-> accepted gaps/failures
-> completeness finalizer
```

Representative domains include reaching values/types, narrowing, completion, closure capture/freshness/invalidation, effects, definite assignment, and coverage. An unknown domain/fact family is a typed unsupported obligation, not an ignored enum variant.

# 3. Demand plan

A deterministic `FlowDemandPlan` records:

- graph/body identity;
- source/input and semantic profile basis;
- result contract;
- selected subject/program point;
- required domains;
- initial and expanded obligation IDs;
- deterministic work order/tie breakers;
- convergence and resource policy.

Planning occurs once per cold logical demand by default. Replanning or lowering a second carrier requires a predeclared measured exception.

# 4. Obligation ledger

Each required obligation has a private typed state, for example:

```rust
enum ObligationState {
    Pending,
    Running,
    Discharged(DischargeEvidence),
    Gap(FlowGap),
    Failed(FlowFailure),
}
```

Expansion may add obligations only through registered deterministic rules. The ledger records origin, required domain, graph node/edge basis, dependencies, and discharge evidence. No public caller can mark an obligation discharged.

# 5. Complete-result construction

```rust
enum FlowSolveOutcome {
    Complete(CompleteFlowResult),
    Partial(PartialFlowResult),
    NoValue(FlowFailure),
}
```

`CompleteFlowResult` has a private constructor. The finalizer succeeds only when:

- graph, demand, profile, input basis, and result contract match;
- the closed domain registry is known;
- every required/expanded obligation is discharged;
- every required fixed point converged deterministically;
- every semantic suboperation is complete under the same contract;
- no gap, stale basis, cancellation, budget exhaustion, panic, or internal failure occurred.

An authored `any` is a semantic value. “Verter has no model” is a gap and cannot become `any`.

Only proof-bearing complete results may be warm-admitted. Partial values may be returned to explicitly tolerant consumers but remain return-only unless a separate operation contract proves partial admission safe.

# 6. Atomic production cutover

`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.

`D2` is one indivisible accepted cutover:

1. route every public effective-flow operation to the new solver;
2. delete the old syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and migration guards;
3. return typed gaps for mechanisms not yet implemented;
4. preserve every ratified Supported/Stable capability and failure contract;
5. prove no second selectable evaluator remains.

Later blocks expand only the sole solver.

# 7. Required tests

- compile-fail/private-constructor proof;
- mutation test dropping one obligation cannot yield `Complete`;
- unknown edge/domain produces a gap;
- plan/order randomized but observable result deterministic;
- no-flow path allocates no graph/plan;
- structural authored returns independent from endpoint completion;
- closure effects independent of expression position;
- loop/completion convergence and budget failure;
- partial replay never appears as warm complete;
- source search, dependency graph, and runtime tests prove the legacy evaluator is absent after `D2`.
