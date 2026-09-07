# D3P flow-graph node-enumeration surface amendment (rev11.flow)

- Status: accepted (ratified by the maintainer, 2026-09-07, after architect review)
- Date: 2026-09-06
- Revised: 2026-09-07 to record the reviewed enumeration contract and cutover requirements
- Amends: `charters/rev11-flow/D3P.md` production-surface statement and production-file
  list, with owner-level enumeration proof
- Scope: D3P only; no other node's charter, budget, or ledger changes
- Landing: D3P lands as a member of the stacked D3 chain (D3R -> D3I -> D3P -> D3C) in
  one squash; no standalone merge.

## Context

D3P's charter states "Production surfaces: `crates/verter_session/src` only" and lists
four production files, all under `project_semantic_dispatch/`. The candidate adds nine
production lines outside that list:
`crates/verter_semantic/src/analysis/flow/flow_graph.rs`, one accessor
`FunctionFlowGraph::node_at(index) -> Option<FlowNodeId>`.

The current product substrate enumerates one product slot per `(requested domain, graph
node)` pair. This motivates a graph-owned enumeration boundary; it does not establish
that every demand must compute the entire product universe. The live cutover must follow
D3C's existing contract: selected transfers in `FlowDemandPlan` order, with the selected
obligation frontier and `max_iterations` as the connected budget.

`FunctionFlowGraph` publishes
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
   `crates/verter_semantic/src/analysis/flow/flow_graph.rs` is added to D3P's
   production-file list for node enumeration only: the existing `node_at` and the
   `nodes` iterator contract below. The charter's "`crates/verter_session/src` only"
   sentence is read as amended by that list. Enumeration proof may extend the existing
   owner test file, `crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs`.
   No other semantic production surface is added.

3. **The amendment is bounded.** It stays inside D3P's declared
   `max_related_packages = 2`, inside the `flowslice` conflict domain, and inside the flow
   substrate (`crates/verter_semantic/src/analysis/flow`) that the charter itself names as
   the final owner's substrate. It adds no second flow engine, no second graph
   representation, and no resolution or type surface.

4. **The primary enumeration API expresses iteration.** The long-term surface is
   `FunctionFlowGraph::nodes() -> impl ExactSizeIterator<Item = FlowNodeId> + '_`.
   It yields every valid node exactly once in ascending graph-local index order,
   including local and captured bindings, expression sites, return sites, and control
   regions. The contract extends to every graph-owned node family; captured binding
   construction remains the prerequisite owner's responsibility. Creating
   and advancing the iterator take constant time and allocate nothing; a full traversal
   is O(node_count) with constant auxiliary space. The graph alone mints its ids.
   Migrate enumeration callers to this surface and remove `node_at` in the same change
   unless a real indexed-lookup caller remains. Keeping indexed lookup for such a caller
   is compatible with this contract; an enumeration caller is not that justification.
   The existing `node_at` is a valid constant-time, allocation-free owner accessor, so
   ratification does not require deleting it ahead of its consumer migration.

5. **Enumeration does not determine evaluation scope or graph identity.** The graph
   retains the complete structural inventory. Computed product slots come from the
   sealed demand plan's dependency closure, including required value, effect, and
   control dependencies. A numeric `FlowNodeId` is local to one graph; successful
   enumeration or an in-range index check proves neither cross-graph nor cross-version
   compatibility. Product inputs, seed construction, and stores must preserve the
   content-pinned graph scope supplied by `BoundFlowGraph`.

## Product cutover requirements

The architect review identified two product-boundary issues adjacent to this accessor.
Their disposition is **ADOPT-NOW within the existing atomic D3 candidate**: D3P owns the
product substrate correction; D3C owns its integration with the existing sealed plan and
finalizer. These are requirements before the atomic candidate lands, not follow-up debt,
and they do not change D3C's charter or grant an additional flow authority.

- **Demand-selected work.** `solve_flow_products` currently budgets and mints the entire
  requested-domain by graph-node universe. Its live use must instead consume the sealed
  plan selection through the same product kernel. Unrelated, unselected nodes must not
  create product slots, consume the selected-product budget, or fail the demand merely
  because their bindings are unmodeled. Required effect and control dependencies remain
  selected; this is not permission to drop them or manufacture an incomplete closure.
- **Graph-scoped products.** `FlowProductKey` currently carries a domain, local node id,
  and optional binding identity. Nonbinding keys from different graphs can therefore
  compare equal, while `FlowProductContext::new` accepts inputs and seeds without a
  graph-scope check. Bind seed construction and solved stores to the same content-pinned
  graph as the inputs, and reject foreign keys and graph/version mismatches or prevent
  their construction through scoped capabilities. Validate compatibility at attachment
  boundaries rather than repeatedly hashing full graph identity per edge. Compact
  graph-local ids remain appropriate; they must not be mistaken for global identities.

For performance, use compact selected-product slots and deterministic scheduling, reuse
graph/plan-owned adjacency, and avoid repeated target collection/sorting and unconditional
cloning of full visitation keys. The current tree key universe, per-domain reverse
adjacency reconstruction, and per-round collections require explicit work/allocation
evidence before becoming the live path. Reusable structural indexing belongs to the
existing graph or plan owner, not a second graph representation. An iterator change alone
is not evidence of a meaningful solver speedup. Existing canonical semantic algebra,
typed degradation, and finalization authority remain unchanged.

## Verification contract

Extend existing focused coverage where possible; each new assertion must discriminate a
plausible contract failure.

- Owner enumeration: every valid node appears exactly once across all graph-owned families,
  empty graphs yield no nodes, and enumeration agrees with the family constructors and
  CSR edges. If `node_at` remains, `node_count()` and `usize::MAX` return `None`.
- Product scope: foreign-graph and foreign-version keys/seeds cannot affect another
  graph's solve, including nonbinding nodes with identical local indices.
- Demand isolation: adding unrelated, unselected graph content preserves the result and
  selected product work; required effect/control dependencies continue to participate.
- Preserve permutation determinism and the exact budget-exhaustion boundary, including
  the guarantee that degraded outcomes retain no warmable candidate.
- Show bounded selected-product work and allocation behavior using the owning counters,
  inspection, or benchmarks. Account separately for once-per-version graph construction
  and per-demand product work; use the existing performance methodology and budget owner.

## Alternatives rejected

- **Fabricate `FlowNodeId`s in `verter_session`.** Requires a public constructor or
  public per-family counts, and moves the graph's index-layout invariant into a consumer.
  Strictly worse than one read-only accessor on the owner.
- **Enumerate only binding nodes.** The product lattice is defined over the whole node
  space (expression sites and return sites carry reaching values; regions carry guard
  facts), so the owner API must cover all graph-owned families. This does not require every
  query to evaluate all nodes.
- **Treat dense indexing as the only enumeration API.** `node_at` is valid indexed
  lookup, but forcing every enumeration caller to pair it with `node_count()` adds a
  fallible lookup to an intrinsically valid traversal. The owner iterator expresses the
  required operation directly without exposing per-family layout.

## Ratification status

The maintainer explicitly requested ratification and publication on 2026-09-07 after
the architect review. This revision records that decision, the graph-owned enumeration
contract, and the requirements that remain before the product solver becomes live.

The accessor was implemented while the original record was still proposed. That order
is retained as history; the ratification date is 2026-09-07 and is not back-dated.

The production-surface ratification gate is cleared. This is acceptance of the revised
architecture decision, not a claim that its iterator migration, product corrections,
verification obligations, or the complete stacked candidate have passed. No ledger row,
budget, or implementation-completion status changes with this ratification. The
D3R -> D3I -> D3P -> D3C chain still lands as one squash after its applicable gates pass.

### Precedent

This is the second CROSS-CRATE production-surface amendment in the stacked
chain, and the first was ratified.
`2026-09-02-d3r-typeof-carrier-mint-site-and-test-home.md` amends the same
charter sentence — D3R's "production surface is `crates/verter_session/src`
ONLY" — for a deviation into the same crate this one enters. D3R's ratified
ask is the larger of the two: its candidate changes four files under
`crates/verter_semantic/src` (+178 / -8), and its ratification section names
the deviation as cross-crate explicitly rather than folding it into the
package count. The maintainer ratified it on 2026-09-05, and it carries
`Status: accepted (ratified by the maintainer, 2026-09-05)`.

The precedent is coordination context, not authority for this decision. Ratification
comes from the maintainer's 2026-09-07 instruction. The architectural justification is
that the graph owns its node space; neither a package-count allowance nor the size of a
previous amendment establishes that ownership. The semantic-crate behavior added here
is the read-only node-enumeration API described above.
