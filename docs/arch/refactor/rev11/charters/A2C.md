# A2C — Retired completion predecessor

**Status:** SUPERSEDED by AMD-004; retained only as a historical DAG/ledger row.  
**Class:** Foundational safety, historical.  
**Predecessors:** A2.  
**Successors:** None.

## Disposition

A2C is not executable. It has no accepted candidate and may not re-enter `READY`,
`IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.

The rejected eager-skeleton candidates and the incomplete V3 implementation remain
unlanded historical evidence. Their correctness, performance, mutation, and test
results do not transfer to another block.

Exact structural completion and G10 discrimination are deferred to D6 /
`U6.LOOP_CLOSURE` under debt row `FR-D8` in
`docs/arch/u6-flow-return-gaps-and-target.md`.

## Preserved architecture constraints

- `FunctionBodySkeleton` carries content-free canonical topology only.
- The demanded `FunctionFlowGraph` is the sole completion reducer.
- Completion meaning is not reconstructed from statement syntax.
- A3 consumes only typed `FlowGap` information from an owning producer.
- No syntax-only G10 detector, second completion classifier, second graph,
  target-indexed completion set, fixed target ceiling, or A3-only retained payload
  may be introduced.
- Checker-correct clean/warm cases must not be refused to make completion evidence
  appear safe.

## Resume condition

Heavy structural-completion work may resume only under D6 after its implementation
lock contains a closed, code-first inventory of every completion producer, transient
carrier, constructor, transfer, discharge route, result-assembly input, publication
exit, and admission exit. The inventory must be pinned to one checkout and proven by
real-path tests and transfer-site mutations.

## Exit criterion

There is no implementation exit criterion. The live A2C ledger row closes terminally
as `SUPERSEDED` under AMD-004.
