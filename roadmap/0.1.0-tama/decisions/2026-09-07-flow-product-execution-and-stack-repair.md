# Flow-product execution and stacked integration clarification

- Status: accepted for implementation under the maintainer's 2026-09-07 direction to repair the affected stack; implementation and final review remain required
- Date: 2026-09-07
- Applies to: the existing D3I prerequisite, D3P product substrate, and D3C live cutover
- Landing: preserve the owning stacked PRs and the existing atomic landing requirement

## Context

The ratified node-enumeration amendment requires selected, content-scoped products.
Source conformance inspection found that the stable binding prerequisite is absent
and that the live cutover replaces the graph-bound substrate with name-interned frame
slots. Correcting the substrate alone would therefore leave its fixes unused.

`FunctionFlowGraph` records dependencies and selection, not control-flow predecessors.
A binding hub can refer to multiple writes in source order; joining every dependency
would retain overwritten definitions. Region membership likewise does not determine
which control-flow continuations reach a merge.

## Intent contract

The existing selected `SliceRegion` evaluator remains the sole execution-order driver,
including abrupt exits, switch fallthrough, exception paths, and finally continuations.
The shared product owner supplies graph-scoped selected storage and exhaustive transfer
and join operations. Every live product mutation and actual continuation join uses that
owner. Dependency edges never substitute for incoming execution states.

`FlowDemandPlan` bounds eligible work and provides the declared tie-break among
simultaneously ready operations. Source control order and semantic readiness precede
that tie-break. `max_iterations` bounds actual fixed-point rounds, not statement count
or repeated idempotence checks over unchanged predecessor snapshots.

## Changes

- Restore the complete indexed binder inventory and exact skeleton-to-index mapping.
  Local references carry skeleton binder identity; captured references carry the exact
  defining function and binder identity. Text names remain diagnostic metadata.
- Construct production graphs only from the indexed prepared skeleton/binding pair.
  Read, write, and callee references never recover identity by name. The lexical index
  resolves containing scopes, with parameter defaults separate from body declarations.
  Structural selections, retained demand/hash pairs, and work measurements are sealed
  by their owning constructors; consumers receive immutable views.
- Preserve already-supported nested callable forms by extending the existing function
  index and locator owner. A child obtains its own graph and sealed demand from shared
  cache/planning owners. Captured inputs are imported by exact identity at selected
  captured-binding nodes into that child scope; parent keys and transient narrowing do not
  cross the boundary.
- Retain content-free defining-frame descriptors and exact annotation presence/spans.
  Hydrate only selected authored annotations under the original pinned snapshot and
  generic binder environment, and retain them in the original source declaration bank.
  Prepare immutable selected child inputs before sealing their demand basis. Effect-only
  captures do not force runtime value hydration. Binding-value obligations expand
  only for value-selected subjects. Each closure expression site records its exact
  read or effect requirement, even when another site reads the same shared hub.
  Effect-only capture evidence attests the completed selected structural execution
  and closure dependency without fabricating reaching-value or assignment products.
- Use compact slots over selected subjects, with immutable metadata shared by execution
  snapshots. Keys, inputs, snapshots, and evidence are bound to the graph content,
  demand basis, and execution. Unrelated graph inventory does not allocate products
  or consume the product budget.
- Keep continuation values in a persistent ordered store. Snapshot cloning shares the
  tree; writes copy only changed paths, and iteration and joins visit materialized cells
  in domain then selected-subject order. Multiway joins merge ordered predecessor
  streams with one queued head per predecessor; absent cells are accounted for by
  each domain's bottom rule, without scanning the predecessor-by-subject cross product.
  Retained declarations use exact source cells
  in one append-only execution authority bank. Hoisted aliases share runtime state,
  while each authored annotation retains its own identity and explicit lookup fallback.
  `max_products` bounds runtime cells in one continuation; `max_declared_products`
  separately bounds source facts over the execution lifetime. Their sum bounds all
  cells visible through any snapshot. Sharing a declaration bank must not silently
  invalidate a previously admitted continuation's runtime budget.
  Execution capabilities are thread-local (`Rc`); immutable graph and selection
  artifacts remain shareable (`Arc`). Atomic publication performs no callbacks,
  suspension, or interpreter reentry, and execution handles cannot cross workers.
- Separate validated structural execution from obligation expansion. The sealed
  `FlowExecutionSelection` retains the original content and selected demand. A proof
  plan wraps that same capability; only the exact matching plan can obtain product
  discharge evidence. Value-only execution after a proof refusal preserves the existing
  cold-result finalizer and cannot manufacture a successful proof plan.
- Admit numeric node and binding addresses only at the content interpreter's pinned
  graph attachment. Public product handles retain their originating execution scope.
- Apply explicit executed transfers transactionally. Binding writes invalidate their
  matching narrowing facts; the presence of a write dependency elsewhere is not an
  executed write. Successful unchanged work contributes exact domain/subject evidence.
- Join the actual incoming continuation snapshots once through exhaustive domain
  joins and the canonical semantic algebra. Preserve reaching definitions, declared
  types, definite-assignment metadata, member-path narrowing, and canonical widening
  provenance. A failure exposes no partially accepted result or completion evidence.
- Inspect literal widening provenance in one bounded canonical-owner operation with
  shared payload reads, semantic literal hashing, and inspection-root evidence. Pinned
  occurrences win; surviving all-fresh literals use the compact `All` representation.
  Actual multiway reaching-type joins aggregate source-ordered contributors once and
  make one canonical union/provenance operation. The inspection budget applies to
  that actual join, rather than resetting at each temporary binary prefix. Semantic
  literal hashes are computed once per inspected payload within the operation.
  Union literal subsumption collects base primitive presence once before filtering;
  a literal-only union never rescans all members for each literal.
- Represent captured binding dependencies with real graph-owned captured-binding
  hubs. Exact captured identities are distinct from local skeleton binder IDs; hubs
  avoid a read-by-write cross product. Static read paths compose with write paths in
  the shared peeker, including captured writes and their governing control inputs.
  Bound projection states explicitly so growing paths terminate with typed refusal.

## Completion routing and checker inference

The 2026-09-08 integrated review confirms that clause outcomes must retain their
evaluated normal continuation and pending completion states. Syntax-only fallthrough
cannot revive a terminated state. Evaluate `finally` through the existing interpreter
for each corresponding incoming completion; a broadly joined normal-and-exceptional
endpoint is not a transferable result of a conditional clause. Preserve narrowing
from actual surviving states instead of restoring guards killed by executed writes.
Switch refinement updates reaching type and the matching narrowing position atomically
without inventing an assignment or discarding unrelated member facts.

Clause-scoped assertion cleanup projects entry narrowing facts only onto runtime
roots without executed write or invalidation receipts since that clause's entry.
An unchanged write or later assertion cannot restore an invalidated entry guard.
Conversely, strengthening a guard without a write must not discard the valid outer
guard when leaving the clause. The shared kernel normalizes aliases, validates all
scopes and product capacity, and publishes only the narrowing replacement atomically;
reaching values, reaching types, assignment state and authored authority remain intact.
This projection grants no execution evidence.

The result contract remains checker type inference, including its existing authored
return and abrupt-break policy. A pending break may carry an explicitly typed checker
snapshot projected from the same execution, alongside the actual completion state.
An outer `finally` reads the actual state; the matching inference target reads the
checker projection. Scope closure applies to both. A replaced pending completion must
not revive a runtime exit. Conservative entry and catch-consumed throw states may
participate in authored-return inference only as tagged checker-only lanes. Their
products, walks and child results cannot supply missing actual execution evidence or
rescue an aborted required lane. This projection uses the same product authority;
it is not an independently mutable inference store or a second interpreter.

Result-contract descriptor v6 explicitly registers `AuthoredFinallyReturn` and
`OverriddenBreakSuffix` as checker return capture-processing rules for `FlowReturn`.
The closed policy is encoded in the result identity. Exact capture receipts retain
their selected parent expression site, capture identity and observation purpose.
Checker route permits are private to the actual control-policy seams; their receipts
are staged until the successful return route contributes to the checker result.
Only an Effect-capture requirement may accept such a receipt, and only alongside a
completed required runtime walk. It never proves an actual runtime event, Value
product, call or relation. Another site, discarded route or truncated required walk
cannot borrow this permission. Distinct continuation purposes survive enclosing region
boundaries; normal outputs with incompatible inference policies cannot be merged and
relabelled as runtime execution.

All completion evaluations share the same selected IR, execution capability,
declaration bank and sticky failure state. Interpreter work has a monotonic limit
separate from fixed-point iterations. Live owned completion frontiers have an
execution-global capacity with RAII reservations acquired before storage growth and
released when records are drained or dropped. Nested frontiers count together.
Neither projection entry nor continuation replay resets either budget.

Installed declaration fallback is indexed by the existing runtime storage group,
with exact source authority first and source-order fallback among installed cells.
Multiway reaching definitions are merged once under their distinct-width limit;
neither operation repeatedly reconstructs or scans an accumulated prefix.

## Legacy deletions

Remove the whole-graph dependency propagation solver, name-interned product subjects,
private semantic frame unions, repeated idempotence rounds, fabricated binder slots,
and name-based substitutes for exact product evidence. Retain one product store, one
transfer route, one join route, and the existing control interpreter.

## Verification

Use regressions for exact binder inventory and shadows, sequential overwrite, actual
conditional continuations, graph/version/demand isolation, selected-demand budget
isolation, atomic failure, successful unchanged evidence, nested function scope, and
widening provenance. Run the relevant semantic and live flow suites, then the owning
repository gates and independent architecture, adversarial, and performance reviews
against the integrated stack. Existing unsupported capabilities keep their typed gaps.
Measure snapshot, write, iteration, and join allocations across increasing selected
capacity and fixed materialized-product counts. Snapshot clones must allocate nothing;
unused selected capacity must not increase continuation-operation allocation cost.
