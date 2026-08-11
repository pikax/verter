## Verdict

**BLOCKING FINDINGS. Candidate `04048a…` must not land. A2C is not viable as currently chartered.**

The premise I specified was wrong: a query-specific root endpoint verdict should not be eagerly composed during every `FunctionBodySkeleton` construction. The skeleton should eagerly index canonical control facts; the demanded flow-graph build should derive completion meaning.

AMD-002 must be reopened. G10 structural discrimination should move to D6’s sole completion/flow-graph authority, delivered early through the existing A2C predecessor slot. A3 should consume a typed `FlowGap::AbruptCompletion`, not a second endpoint classifier.

## Findings

### 1. The eager derived-fact premise is wrong

The current design computes a derived A3 verdict even for recursively constructed nested skeletons used only for capture inspection. Those paths exist at [flow/mod.rs:1313](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:1313) and [flow_slice_content.rs:2508](<REPO>-wt-a2c/crates/verter_session/src/flow_slice_content.rs:2508). Their completion endpoint is not consumed.

The correct split is:

- Eager during the syntax walk: canonical, content-free control indexing.
- Demand-time during the already-required graph build: completion edges, target routing, and the root verdict.
- Never: AST retention, AST rewalk, a separate completion memo, or a syntax-shaped A3 classifier.

Thus the broad-indexing premise remains sound; eager semantic composition does not.

### 2. The candidate is not actually linear on target depth

The target-heavy cost is not merely “the irreducible price of exactness.”

[labels_before_iteration](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:1976) walks the remaining nested-label chain. It is called again for every enclosing label at [flow/mod.rs:2141](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:2141). For 64 nested labels this performs approximately:

```text
63 + 62 + ... + 1 = 2,016
```

label inspections: **O(depth²)**.

Additional target costs are:

- initialization of `[None; MAX_COMPLETION_TARGETS]` on every skeleton at [flow/mod.rs:1149](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:1149);
- one push, pop, and `CompletionSet` route per label;
- repeated copying/composition of the 24-byte target-bitset set.

The purported linearity test at [completion_skeleton_tests.rs:253](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/completion_skeleton_tests.rs:253) tests only flat expression statements. Its manually incremented `completion_work_units` does not count the repeated label-chain scan. The required linearity evidence therefore also fails.

Consequently, the 72–78% result is decisive against this candidate but is **not** a lower bound for every possible completion design.

### 3. Target identity cannot be eliminated

No exact root-only algebra can discard target identity entirely. X80 and X88 differ precisely in whether `break OUT` or `break INNER` is routed. Erasing that identity makes them observationally indistinguishable.

What can be dropped:

- `MAX_COMPLETION_TARGETS`;
- `CompletionTargetId`;
- the fixed active-target array;
- `breaks: u64` and `continues: u64`;
- per-statement `CompletionSet` composition;
- the 64/65 capacity discontinuity;
- endpoint storage in root-region flags;
- the entire eager `CompletionDraft` evaluator.

What must remain somewhere:

- canonical label/switch/loop identities;
- authored `return`, `throw`, `break`, and `continue` events;
- exact break/continue destination edges;
- try/catch/finally grouping and override edges;
- statement-list/region ordering sufficient to determine the root continuation.

These belong in the sole control graph, represented by dense IDs and direct edges—not as target-indexed completion sets.

The reduced graph algebra decides:

```text
CompletionCoverage =
    Exact {
        authored_return_present,
        root_normal_reachable
    }
  | Unknown(CompletionGapReason)

endpoint =
    no authored return          => DoesNotContribute
    Unknown(reason)             => Unknown(reason)
    root normal reachable       => Contributes
    otherwise                   => DoesNotContribute
```

It also preserves the pinned TypeScript inference behavior for `finally` by representing the prior-break continuation and finalizer override as explicit graph edges. It does not answer arbitrary per-statement completion-set queries.

### 4. The measured fraction of a realistic cold request is unknown

The evidence measures only `build_function_body_skeleton` over an already-parsed body. It contains no public cold-request timing, so any claimed realistic percentage would be fabricated.

The measured lane medians for the target cells are:

| Shape | Baseline skeleton | Candidate skeleton | Added time |
|---|---:|---:|---:|
| 64 targets | 2.544 µs | 4.399 µs | 1.855 µs |
| 65 targets | 2.491 µs | 4.400 µs | 1.909 µs |

For a cold request taking `L` microseconds:

```text
baseline skeleton fraction = 2.544 / L
A2C incremental fraction   = 1.855 / L
```

`L` was not measured. That omission must be corrected before another performance decision.

### 5. Gate ruling

The 3% construction gate was not improper for the rejected universal eager computation. It correctly exposed work amplification that every skeleton user would pay. Replacing it only with end-to-end latency would allow batch CPU and nested-skeleton amplification to disappear inside a larger denominator.

However, it is the wrong sole acceptance cell for the replacement architecture because the measured operation and ownership boundary change materially.

The successor needs three cells:

1. **Universal skeleton indexing**

   Representative-corpus aggregate skeleton construction, including functions whose skeletons are built only for nested capture analysis. Keep the no-regression bound:

   ```text
   upper slowdown <= max(3%, 2 × measured noise floor)
   ```

2. **Demanded cold flow request**

   The public `get_flow_return_type_with_audit(...whole_return())` boundary, including G10, X05, X68, X80, X88, switch/catch siblings, and 64/65/deep-target cells. Use both:

   ```text
   relative upper slowdown <= max(3%, 2 × measured noise floor)
   absolute cold-request SLO = frozen before successor implementation
   ```

3. **Adversarial work amplification**

   Assert graph/index work is `O(control constructs + abrupt events + emitted edges)`. No hidden depth² scan, fixed target-capacity transition, or repeated syntax traversal. Report absolute nanoseconds and bytes per indexed control/event; freeze their numeric bounds before the successor candidate exists.

This is a benchmark-definition change caused by a false ownership/equivalence premise, not a relaxation for `04048a…`. All current candidate performance evidence remains historical failure evidence and cannot be reused as successor acceptance evidence.

## Alternative placements

- **Compute during the existing skeleton walk:** already attempted. Indexing there is correct; composing the derived endpoint there is not.
- **Derive from currently retained skeleton facts:** impossible. Regions exist, but exact abrupt event identity, label ownership, and try/catch/finally grouping are insufficient for X80/X88 and the sibling cases.
- **Extend the skeleton’s structural index, then derive in `build_function_flow_graph`: recommended.** This retains owned content-free facts, performs no AST rewalk, and charges graph reduction only to actual graph demand.
- **Change what A3 asks: recommended.** A3 should ask whether the result carries an abrupt-completion gap, not compare an independently computed endpoint fact.
- **Leave G10 until full D6:** unacceptable. It knowingly leaves a catalogued wrong-and-warm result alive.
- **Optimize the current O(depth²) implementation and retry:** strongest counter-argument. It could materially reduce the benchmark result, but even if it passed, it would leave a disposable A3-specific derived classifier beside D6’s eventual graph authority. That is not the best durable design.

## Exact replacement specification text

Replace the A2C charter with:

> # A2C — Completion topology and G10 safety verdict
>
> **Class:** Foundational safety; early structural slice of D6’s sole flow-graph authority.  
> **Predecessors:** A2.
>
> ## Objective
>
> Extend the canonical function skeleton with the minimum content-free control topology required by the sole `FunctionFlowGraph`, then derive abrupt-completion coverage during the existing demanded graph build. Supply A3 with a typed `FlowGap::AbruptCompletion` when the current producer’s endpoint contribution contradicts the graph verdict or the verdict is unknown.
>
> ## In scope
>
> - Canonical control-construct identities and parent/group relationships for labels, switch, loops, try, catch, and finally.
> - Source-ordered content-free completion events for return, throw, labeled/unlabeled break, and labeled/unlabeled continue.
> - Direct graph edges for normal continuation, return, throw-to-catch, break/continue destination, switch exit, and finally preservation/override.
> - Structural authored-return membership.
> - One exact-or-typed-unknown root completion-coverage verdict stored on `FunctionFlowGraph`, not `FunctionBodySkeleton`.
> - A typed `FlowGap::AbruptCompletion` emitted by the flow producer when its endpoint-undefined claim disagrees with that verdict or coverage is unknown.
> - Exact G10, X05, X68, X80, X88, switch/catch sibling, malformed-target, deep-target, and non-interference evidence.
>
> ## Out of scope
>
> - Value typing, capture/effect transfer, freshness, or escape; D5 remains owner.
> - Loop fixed points, slot-state transfer, narrowing, or final flow joins; later D6 work remains owner.
> - Proof-carrying complete-result construction and cache-admission closure; D8 remains owner.
> - AST retention, query-time AST rewalk, a completion memo, a second syntax evaluator, target-indexed completion sets, or a fixed target-count ceiling.
>
> ## Construction contract
>
> Skeleton construction performs one syntax walk and records only canonical structural topology/events. It does not compute or retain `EndpointUndefinedFact`, `CompletionSet`, statement completion sets, suffix completion sets, or active-target bitsets.
>
> `build_function_flow_graph(&FunctionBodySkeleton)` is the sole completion reducer. It resolves completion events to dense control identities, emits completion edges on the existing graph, and computes `CompletionCoverage`. No other production component interprets label, switch, loop, try/catch/finally, break, continue, throw, or return composition.
>
> ## A3 contract
>
> A3 consumes only the producer’s typed degradation:
>
> ```rust
> match flow_result.degradation() {
>     Some(FlowGap::AbruptCompletion { .. }) => {
>         // Partial/FlowGap/NoValue; suppress warm admission.
>     }
>     _ => {}
> }
> ```
>
> A3 must not read statement syntax, skeleton regions, completion events, graph edges, or an endpoint accessor.
>
> ## Required performance evidence
>
> - Representative-corpus aggregate skeleton-index construction passes the frozen 3%/noise gate.
> - Public cold flow requests pass the frozen relative gate and a predeclared absolute SLO.
> - Work is linear in indexed control constructs, completion events, and emitted completion edges.
> - No fixed target-capacity discontinuity; 64 and 65 live targets are ordinary exact inputs.
> - Retained bytes are attributed solely to canonical D6-required topology; no A3-only retained payload exists.
> - No completion-owned allocation occurs for functions containing no completion-relevant control/event beyond data already required by the skeleton.
>
> ## Abort/rescope
>
> Stop if exact discrimination requires value typing, D5 effects, loop fixed points, D8 proof minting, AST retention/rewalk, a second graph, or a second completion classifier.

## Exact source disposition

For the successor based on parent `70ea4c…`:

- Do not carry forward the endpoint encoding at [flow/mod.rs:227](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:227).
- Delete the current `CompletionSet`, `CompletionTargetId`, `MAX_COMPLETION_TARGETS`, and eager composition from [completion.rs](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/completion.rs:1).
- Delete `active_completion_targets`, `completion_overflowed`, `labels_before_iteration`, and all `visit_*_completion` paths beginning at [flow/mod.rs:1116](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:1116).
- Keep one skeleton visitor. Extend its canonical structural index with dense control identities and source-ordered abrupt events.
- Generalize the existing return-site index at [flow/mod.rs:606](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/mod.rs:606) into the sole completion-event index; existing return consumers filter `Return`.
- Extend [build_function_flow_graph](<REPO>-wt-a2c/crates/verter_semantic/src/analysis/flow/flow_graph.rs:243) with completion nodes/edges and `CompletionCoverage`.
- The later D6 implementation must extend these same graph edges; it may not rebuild completion meaning from syntax.
- A3’s specification and charter must replace “consume accepted A2C endpoint facts” with “consume the typed abrupt-completion degradation produced by the sole D6 graph authority.”

## Distinct maintainer-ratification items

1. **Reopen AMD-002.** Ratify a new amendment superseding AMD-002 points 2–4. Keep the DAG lineage `A2 → A2C → A3`, but redefine A2C as an early structural delivery of D6’s sole completion-graph authority.

2. **Move G10 ownership.** Move structural G10 discrimination from an independent A2C skeleton-fact owner to D6’s completion graph. A3 owns only retraction/non-admission in response to the typed gap.

3. **Recalibrate the performance cells.** Retire the five per-shape skeleton-relative cells as the sole acceptance instrument. Retain them as diagnostics, add the aggregate universal-index and public cold-request cells above, freeze absolute limits before successor implementation, and restart all affected evidence.

4. **Invalidate the candidate.** Preserve `04048a…` and its digest-verified bundle as failed historical evidence. Restart implementation from `70ea4c…`; no approval, mutation result, or latency result transfers.

__DONE__
