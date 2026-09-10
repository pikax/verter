# Flow product execution and determinism contract

- Status: accepted (maintainer-authorized correction of valid external review findings)
- Date: 2026-09-08
- Amends: the execution and determinism clauses of D3P and D3C, the D3 split
  decision, and the D3P enumeration decision. Other ownership and landing rules remain.

## Execution authority

`FlowDemandPlan` seals the selected executable universe and proof obligations.
`FunctionFlowGraph` is a dependence graph: its edges select required work; they
do not establish runtime predecessor states or an executable control-flow order.

The existing control interpreter executes source-ordered transfers and supplies
the actual continuation snapshots at each join. `FlowProductExecution` owns
selected, graph-scoped product storage, transfer validation, domain joins,
resource accounting and exact executed-domain evidence. `FlowFrameProducts`
adapts the interpreter to that kernel without a second lattice or semantic map.
Canonical type algebra remains with the existing semantic algebra owner.

The interpreter reports actual fixed-point rounds. Acyclic joins consume no
rounds; exhausting the plan's iteration or selected-product budget follows the
existing typed failure path. Work remains bounded by selected products, actual
transfers and predecessor entries. Immutable graph/selection indexes are shared;
continuation snapshots share persistent runtime storage and the execution-local
append-only declaration authority. No dependence-edge worklist is reconstructed.

`FlowTieBreak::DomainNodeEdgeSlot` retains its role in deterministic obligation
ordering and report application. It does not schedule interpreter statements.
The existing `FlowDischargeReport` application and finalizer remain the only
positive admission authority. Selection alone, a stored product alone, or a
test observation cannot mint execution evidence or a complete result.

This supersedes the earlier requirement to execute transfers in plan order
until the entire dependence graph stabilizes. Reintroducing that scheduler
would conflate dependencies with control flow. D3P owns the kernel; D3C owns
the live interpreter adapter and integration. D4–D7 successor semantics remain
outside this correction.

## Discriminating determinism proof

Equivalent executions must produce canonical-equivalent product snapshots,
exactly the same executed-domain/subject evidence and fixed-point count, equal
discharge entries, equal served results, and the same warm admission outcome.
The observations preserve graph-local subjects in the same pinned content;
they remove execution capabilities and canonicalize semantic contributors,
never compare unrelated graph-local IDs as globally meaningful identities.

- At the kernel, `product_snapshots_and_evidence_are_predecessor_permutation_invariant`
  varies actual predecessor and transfer arrival order across all permutations
  of three continuations. It observes all five product domains, an absent
  definite-assignment input, selected-but-unexecuted sites, and exact evidence.
- At the live boundary, `flow_product_execution_is_permutation_deterministic`
  varies equivalent demand and actual predecessor order. Test-only observations
  compare arena-free product values and the real discharge report, together
  with the served type and a demonstrated warm read. Existing missing-product
  and budget-refusal tests remain the negative admission controls.

Raw statement visitation need not be identical under a deliberately permuted
equivalent execution. The contract is semantic and evidence equivalence, not
an invented scheduler transcript. Test observations have no shipped cost and
do not constitute a second proof authority.

## Verification and scope disposition

The previous isolated passing reruns do not explain the historical exhaustive
fallthrough failure. Run the canonical exhaustive gate once on the final
corrected candidate; investigate the failure if it reproduces. Retain the old
failure in the review history even if the current candidate passes. This is
candidate verification, not a frozen-count requirement or mutation replay.

D3C records its concrete scope-coherence investigation in its owning charter
under `contracts/sizing.md`. Numeric estimates do not justify a mechanical
split. The D3R/D3I/D3P/D3C atomic landing requirement is unchanged; this amendment
does not mark the stack accepted, merged, or a future node implemented.
