# D3P flow-graph node-enumeration surface amendment (rev11.flow)

- Status: proposed — maintainer ratification is a LANDING GATE for this candidate (see
  "Ratification status" below)
- Date: 2026-09-06
- Amends: `charters/rev11-flow/D3P.md` production-surface statement and production-file
  list
- Scope: D3P only; no other node's charter, budget, or ledger changes
- Landing: D3P lands as a member of the stacked D3 chain (D3R -> D3I -> D3P -> D3C) in
  one squash; no standalone merge.

## Context

D3P's charter states "Production surfaces: `crates/verter_session/src` only" and lists
four production files, all under `project_semantic_dispatch/`. The candidate adds nine
production lines outside that list:
`crates/verter_semantic/src/analysis/flow/flow_graph.rs`, one accessor
`FunctionFlowGraph::node_at(index) -> Option<FlowNodeId>`.

The product solve mints one product slot per `(requested domain, graph node)` pair, so it
must ENUMERATE the bound graph's node space. `FunctionFlowGraph` publishes
`node_count()`, `node_kind(FlowNodeId)`, `out_edges(FlowNodeId)` and four per-family
constructors (`binding_node` / `expr_site_node` / `return_site_node` / `region_node`),
but:

- `FlowNodeId`'s inner index field is crate-private to `verter_semantic`, so a consumer
  cannot build an id from a dense index; and
- the four per-family COUNTS (`binding_count`, `expr_site_count`, `return_site_count`,
  `region_count`) are private fields, so the four family constructors cannot be driven
  over the whole space either.

There is therefore no existing enumeration boundary a consumer can reach, and no home in
`crates/verter_session/src` for one: minting the ids there would mean reconstructing the
graph's own dense index layout outside the graph, which is exactly the fabrication the
substrate's key contract forbids.

## Decision

1. **The enumeration accessor lives on the graph, not on the consumer.**
   `FunctionFlowGraph::node_at` is the graph's own statement about its index space:
   dense, contiguous across the four node families, and `None` past `node_count()`. It is
   read-only, allocation-free, and mints no id a caller could not already obtain through
   the family constructors — it only removes the need to know the family layout.

2. **D3P's production surface is amended to include that one accessor.**
   `crates/verter_semantic/src/analysis/flow/flow_graph.rs` (`node_at` only) is added to
   D3P's production-file list. The charter's "`crates/verter_session/src` only" sentence
   is read as amended by that list. Nothing else in `crates/verter_semantic` is touched.

3. **The amendment is bounded.** It stays inside D3P's declared
   `max_related_packages = 2`, inside the `flowslice` conflict domain, and inside the flow
   substrate (`crates/verter_semantic/src/analysis/flow`) that the charter itself names as
   the final owner's substrate. It adds no second flow engine, no second graph
   representation, and no resolution or type surface.

## Alternatives rejected

- **Fabricate `FlowNodeId`s in `verter_session`.** Requires a public constructor or
  public per-family counts, and moves the graph's index-layout invariant into a consumer.
  Strictly worse than one read-only accessor on the owner.
- **Enumerate only binding nodes.** The product lattice is defined over the whole node
  space (expression sites and return sites carry reaching values; regions carry guard
  facts), so a binding-only enumeration would answer a different, smaller problem.

## Ratification status

The accessor is implemented in the candidate while this record is still
`proposed`, which is the wrong order: the record is the authority the code
depends on, so it should have been ratified first. Recorded rather than
quietly reordered, because the alternatives are worse — deleting the accessor
would leave the product solve unable to enumerate the node space it is defined
over (there is no reachable substitute; see "Alternatives rejected"), and
back-dating the record would assert a ratification that did not happen.

What follows from that:

- **Ratifying this record is a gate on accepting the candidate**, not a
  follow-up. A reviewer who reaches the accessor and finds this record still
  `proposed` is looking at an unratified charter deviation, and should say so.
- **The gate is small and read-only.** The whole deviation is one
  allocation-free accessor returning an id the four existing family
  constructors can already produce. It adds no engine, no representation, no
  resolution surface, and no mutation path.
- **Ratification amends this record's `Status` line only.** No charter field,
  budget, or ledger row changes with it; the amendment's content is fixed
  above.
