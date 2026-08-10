# Result Contract and FlightCell Contract

**Status:** Normative reusable-computation and same-key flight contract.

# 1. Query, flight, and cached-candidate identity

```text
QueryIdentity<Q>
  = semantic arguments
  + only profile IDs observed by this typed query boundary
  + ResultContractId

SemanticFlightKey<Q>
  = QueryIdentity<Q> + exact InputBasisId
```

`QueryIdentity<Q>` is the bounded cache-candidate lookup identity. It contains only profiles and contract dimensions observable at this typed query boundary; terminal presentation/serialization is keyed separately when the typed value is unchanged. `InputBasisId` scopes in-flight semantic production but is not part of cross-snapshot candidate lookup. Each cached candidate carries the exact basis, positive/negative read facts, completeness proof, and compatibility material from its production attempt. It is used only after value-side validation against the requester’s current view. A store bounds the number and weight of candidates per query identity; insertion order, newest-basis preference, and global revision are never correctness authority.

`ResultContractId` includes every observable policy that can change what is accepted as a complete result at this typed boundary, without duplicating the separately keyed profile IDs:

- operation/product shape;
- required capability set;
- required exactness/completeness;
- unsupported/degradation policy;
- explicitly requested approximation mode;
- required mapping/diagnostic/serialization outcome where relevant.

Ordinary deadlines, cancellation tokens, trace IDs, priority, and work/time/memory budgets are not reusable result identity.

```rust
struct ExecutionPolicy {
    deadline: Option<Deadline>,
    cancellation: CancellationToken,
    priority: WorkPriority,
    work_budget: WorkBudget,
    memory_budget: MemoryBudget,
}
```

Budget exhaustion is `Partial` or typed failure. It never becomes a weaker `Complete` result.

# 2. Flight classes

## 2.1 Immutable content-artifact flight

Keyed only by exact construction identity. It may be joined across snapshots because its inputs are immutable content/options identities.

## 2.2 Semantic query flight

Keyed by exact `SemanticFlightKey<Q>`—one `QueryIdentity<Q>` and one exact `InputBasisId`. Cross-snapshot joining is disabled by default because the producer’s eventual read set is unknown at join time. Cross-snapshot **warm value reuse** remains possible through value-side validation after completion.

A query family may enable broader in-flight compatibility only through an accepted ADR, a closed proof of compatibility before join, and adversarial retry/cancellation evidence.

## 2.3 Cache-candidate lookup and replacement

- lookup is by exact `QueryIdentity<Q>`, never by request ID, global revision, or current snapshot alone;
- validation occurs before return and before the candidate is consumed as a dependency;
- invalid candidates may be dropped eagerly as hygiene, but correctness comes from value-side validation;
- a new candidate does not automatically erase an older still-valid candidate produced from a different unrelated snapshot;
- multiplicity is bounded by owner policy and retained only when measured reuse value exceeds validation and weight cost;
- candidate choice and eviction are deterministic or semantically invisible; insertion/arrival order cannot affect observable results;
- presentation/serialization candidates are separate from semantic typed candidates when only terminal representation changed.

# 3. Ownership

The `FlightCell`, not the first requester, owns production.

Conceptual states:

```rust
enum FlightState<T> {
    Vacant,
    Running(RunningFlight),
    Finalizing,
    Ready(Arc<T>),
    Failed(Arc<FlightFailure>),
    Cancelled,
}
```

Each waiter has an independent registration, cancellation, deadline, and response slot. The producer continues only while at least one valid waiter remains. If all waiters leave, it cooperatively cancels; useful background work is a separate explicitly requested maintenance operation, not an implicit flight afterlife.

Only `Running` accepts new waiters. `Finalizing`, `Ready`, `Failed`, and `Cancelled` do not. A request arriving after an irreversible completion/failure/budget transition starts or joins a successor flight after normal candidate lookup rather than attaching to the old outcome.

# 4. Join and aggregation

A waiter joins only when exact flight key and result contract match. There is no generic “stronger budget dominates weaker budget” rule.

The cell may aggregate execution policy conservatively:

- effective priority is the maximum active waiter priority, bounded by owner policy, and may lower after the relevant waiter leaves;
- effective producer work/memory budget is the bounded monotonic maximum requested by active waiters, not the sum;
- additional budget may be consumed only while the producer remains `Running` and has not discarded required state;
- ordinary budget cannot choose a semantic approximation, prune required obligations, or change canonical work ordering; explicit approximation is a different `ResultContractId`;
- deadline is not converted into a reusable result contract;
- cancellation of one waiter removes only that waiter;
- when all waiters leave, the owner cooperatively cancels; any useful background completion is a separately requested maintenance operation with its own identity, policy, and bound.

Aggregation cannot change semantics or reinterpret a partial as complete. Budget exhaustion transitions out of `Running`; later higher-budget requests use a successor flight.

# 5. Finalization and admission

Exactly one finalizer records:

- value or typed failure;
- actual exactness/completeness proof;
- positive and negative read facts;
- profile/toolchain/capability basis;
- cancellation/panic/stale/budget state;
- sealed `Publish` or `ReturnOnly` decision.

Every waiter is resolved exactly once. Panic, cancellation, stale basis, resource exhaustion, transient provider/I/O failure, shutdown, and internal failure admit nothing as complete. Followers validate completed values against their current admissible view before use.

# 6. Required state-machine tests

- many followers, one producer;
- first waiter cancels while followers continue;
- all waiters cancel;
- priority elevation and bounded lowering;
- larger budget arrives while `Running` and extends work within bounds;
- larger budget arrives after budget finalization and uses a successor flight;
- ordinary budget cannot select an approximation or change complete output;
- producer panic/failure/shutdown resolves all waiters once;
- double-finalization and self-wait/cycle are rejected;
- incompatible `ResultContractId` does not join;
- different `InputBasisId` semantic requests do not join by default;
- immutable content flight may join across snapshots;
- an unrelated snapshot change still discovers and validates a prior candidate through the same `QueryIdentity<Q>`;
- changed positive or negative facts reject the prior candidate;
- bounded multi-candidate replacement remains schedule/insertion-order independent;
- return-only partial never enters warm cache;
- completed candidate with invalid facts is rejected by a follower.
