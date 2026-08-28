# AMD-003 — A2C completion-graph authority recalibration

**Status:** Registered amendment (maintainer-ratified exception to the normal
verbatim-authority policy — see [`../PROVENANCE.md`](../PROVENANCE.md)).
**Registered in:** [`../README.md`](../README.md) and
[`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md) (R-10).
**Amends:** [`AMD-002-a2c-completion-predecessor.md`](AMD-002-a2c-completion-predecessor.md),
[`../program.md`](../program.md), [`../charters/A2C.md`](../charters/A2C.md),
[`../charters/A3.md`](../charters/A3.md), and
[`../../../u6-flow-return-gaps-and-target.md`](../../../u6-flow-return-gaps-and-target.md).

The published consolidated master, release artifacts, `_EXTRACTION_INDEX.md`, and
historical readiness-review prose remain immutable historical originals. The
digest-verified A2C specifications, stop findings, latency benchmark record, and
command proofs remain valid historical evidence of why the design changed.

## The defect

AMD-002 placed query-specific endpoint composition in every
`FunctionBodySkeleton` construction. Correctness was established, but the eager
derived-fact premise imposed unacceptable universal construction cost and included a
depth-squared target walk. Canonical control facts belong in the eager skeleton index;
completion meaning belongs in D6's sole demanded completion/flow graph.

## The amendment

This amendment **SUPERSEDES AMD-002 points 2 through 4**. AMD-002 point 1 and point 5
onward remain in force.

1. The DAG lineage `A2 → A2C → A3` is retained. `A2C` is redefined as an early
   structural delivery of D6's sole completion/flow-graph authority.
2. Structural G10 discrimination moves from an independent A2C skeleton-fact owner
   to D6's completion graph. Skeleton construction eagerly indexes only canonical,
   content-free control topology and completion events; the demanded graph build
   resolves those events, emits completion edges, and derives the root coverage
   verdict.
3. `A3` owns only retraction and non-admission in response to a typed
   `FlowGap::AbruptCompletion`. It does not interpret statement syntax, skeleton
   regions, completion events, graph edges, or an endpoint accessor.
4. The performance acceptance instrument is recalibrated as specified below. The
   rejected candidate's performance evidence remains historical failure evidence and
   does not transfer to a successor.

## Source disposition

For the successor based on parent `70ea4c…`:

- Do not carry forward the endpoint encoding on `FunctionBodySkeleton` from
  `crates/verter_semantic/src/analysis/flow/mod.rs`.
- Delete the current `CompletionSet`, `CompletionTargetId`,
  `MAX_COMPLETION_TARGETS`, and eager completion composition from
  `crates/verter_semantic/src/analysis/flow/completion.rs`.
- Delete `active_completion_targets`, `completion_overflowed`,
  `labels_before_iteration`, and every `visit_*_completion` path from
  `crates/verter_semantic/src/analysis/flow/mod.rs`.
- Keep one skeleton visitor. Extend its canonical structural index with dense control
  identities and source-ordered abrupt events.
- Generalize the existing return-site index in
  `crates/verter_semantic/src/analysis/flow/mod.rs` into the sole completion-event
  index; existing return consumers filter `Return`.
- Extend `build_function_flow_graph(&FunctionBodySkeleton)` in
  `crates/verter_semantic/src/analysis/flow/flow_graph.rs` with completion nodes/edges
  and `CompletionCoverage`.
- Later D6 work extends these same graph edges and must not rebuild completion meaning
  from syntax.
- A3 consumes the typed abrupt-completion degradation produced by the sole D6 graph
  authority.

No file under `crates/` is changed by this plan-text amendment. The source disposition
above binds the successor implementation.

## Performance instrument

The five per-shape skeleton-relative cells are retired as the acceptance instrument
and retained as diagnostics only. They cannot serve as successor acceptance evidence.
Successor acceptance requires all of the following cells:

- representative-corpus aggregate skeleton-index construction against the frozen
  upper-slowdown gate `max(3%, 2 × measured noise floor)`, including skeletons built
  only for nested capture analysis;
- public cold `get_flow_return_type_with_audit(...whole_return())` requests against
  the frozen relative upper-slowdown gate `max(3%, 2 × measured noise floor)` and a
  predeclared absolute cold-request SLO;
- linear graph/index work in indexed control constructs, completion events, and
  emitted completion edges, with absolute nanoseconds and bytes reported per indexed
  control/event;
- no fixed target-capacity discontinuity: 64 and 65 live targets are ordinary exact
  inputs;
- retained bytes attributed solely to canonical D6-required topology, with no
  A3-only retained payload; and
- no completion-owned allocation for functions with no completion-relevant control
  or event beyond data already required by the skeleton.

The architecture authority must freeze the numeric absolute cold-request SLO and the
numeric absolute nanosecond/byte work bounds in plan text **before any successor
implementation begins**. The ratified recalibration supplies no numeric values for
those limits, so they remain explicit open items. No implementer may invent them or
begin the successor while they are unset.

## Execution precedence

For execution, AMD-003, AMD-002 point 1 and point 5 onward, and the amended live split
files supersede the old completion-ownership text. The executable lineage remains
`A0 → A1 → A2 → A2C → A3 → A4 → A5 → A6`; the DAG and ledger block sets remain
unchanged at 51.

The rejected `04048a…` candidate and its digest-verified evidence bundle remain
immutable failed historical evidence. No approval, mutation result, or latency result
from that candidate transfers to the successor based on `70ea4c…`.
