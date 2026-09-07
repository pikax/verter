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
- Preserve already-supported nested callable forms by extending the existing function
  index and locator owner. A child obtains its own graph and sealed demand from shared
  cache/planning owners. Captured inputs are imported by exact identity at selected
  reference sites into that child scope; parent keys and transient narrowing do not
  cross the boundary.
- Use compact slots over selected subjects, with immutable metadata shared by execution
  snapshots. Keys, inputs, snapshots, and evidence are bound to the graph content,
  demand basis, and execution. Unrelated graph inventory does not allocate products
  or consume the product budget.
- Apply explicit executed transfers transactionally. Binding writes invalidate their
  matching narrowing facts; the presence of a write dependency elsewhere is not an
  executed write. Successful unchanged work contributes exact domain/subject evidence.
- Join the actual incoming continuation snapshots once through exhaustive domain
  joins and the canonical semantic algebra. Preserve reaching definitions, declared
  types, definite-assignment metadata, member-path narrowing, and canonical widening
  provenance. A failure exposes no partially accepted result or completion evidence.

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
