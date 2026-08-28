# AMD-004 — Defer structural completion to D6 and reduce A3

**Status:** Registered amendment (maintainer-ratified exception to the normal
verbatim-authority policy — see [`../PROVENANCE.md`](../PROVENANCE.md)).
**Registered in:** [`../README.md`](../README.md) and
[`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md) (R-11).
**Amends:** [`AMD-002-a2c-completion-predecessor.md`](AMD-002-a2c-completion-predecessor.md),
[`AMD-003-a2c-completion-graph-authority.md`](AMD-003-a2c-completion-graph-authority.md),
[`../program.md`](../program.md), [`../program-dag.toml`](../program-dag.toml),
[`../charters/A2C.md`](../charters/A2C.md), [`../charters/A3.md`](../charters/A3.md),
[`../../../u6-flow-return-gaps-and-target.md`](../../../u6-flow-return-gaps-and-target.md),
and the external live `program-state.toml`.

The published consolidated master, release artifacts, `_EXTRACTION_INDEX.md`, and
historical readiness-review prose remain immutable historical originals. The rejected
completion candidates, V3 specification, specification amendment 1, stop findings,
benchmark records, and failed tests remain historical evidence. None is accepted
implementation.

## The defect

The completion predecessor has failed four times:

1. The first candidate discriminated correctly but retained 10,616 bytes, performed
   157 allocations, and failed the latency gate at 746%.
2. The second candidate retained zero bytes and performed zero allocations but still
   failed target-heavy latency cells at 72–78%.
3. V3 implementation stopped because its purportedly exhaustive transient-carrier
   inventory omitted `DrainedFlowReturnMember`. Specification amendment 1 corrected
   that specification defect.
4. The resumed V3 implementation stopped because sections 8 and 9 contradict each
   other. Section 8 derives `observed` solely from `result.can_fall_through`, but X68
   and X80 contribute `undefined` through `implicit_undefined_seen` while
   `can_fall_through == false`. The required X68/X80 clean result therefore cannot
   satisfy both sections. Nine session tests were written; two pass and seven fail.
   No candidate was committed or accepted.

Repeatedly expanding an ahead-of-code list of completion carriers and semantic cases
is not a finishable prerequisite for A3–A6. Forcing full structural completion through
that critical path would either continue the stop/rewrite loop or weaken false-refusal
discipline. Both outcomes are rejected.

## The amendment

1. `A2C` is retired as an executable predecessor. Its DAG and ledger row is retained
   as a reachable historical row so the validated block universe remains exactly 51
   blocks. The live row becomes terminal `SUPERSEDED`. It may not re-enter `READY`,
   `IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.
2. `A3`'s sole predecessor becomes `A2`. `A2C` is not a predecessor of A3 or of any
   later block. `A4` remains dependent on A3; no other predecessor list changes.
3. A3's exit is reduced to the non-G10 A2-catalogued wrong-complete retractions. Each
   retracted result must use the existing typed degradation/non-admission rails and
   remain cold. Every checker-correct clean/warm preservation row, including X05,
   must remain complete, undegraded, admitted once, and warm on replay.
4. A3 has no G10 obligation. It must not add a syntax-only G10 detector, inspect
   completion syntax for G10, interpret skeleton topology or graph edges, or create a
   second completion classifier. It must not introduce false refusals to compensate
   for the deferred completion authority.
5. Exact structural completion and G10 discrimination become recorded debt owned by
   D6 / `U6.LOOP_CLOSURE`. The debt must close before D6 enters review. A4, A5, and A6
   do not depend on its early completion.
6. Heavy completion work may resume only after the D6 lock contains a closed,
   code-first carrier inventory covering every producer, transient carrier,
   construction, transfer, discharge, result-assembly, publication, and admission
   exit. The inventory must be executable and mutation-discriminating; an open-ended
   prose list amended one missed carrier at a time is not an admissible implementation
   specification.
7. The architectural constraints learned from V3 remain binding:
   - the function skeleton carries content-free canonical topology only;
   - the demanded `FunctionFlowGraph` is the sole completion reducer;
   - completion meaning is not reconstructed from statement syntax;
   - A3 responds only to typed `FlowGap` information supplied by an owning producer;
   - no second graph, completion classifier, target-indexed completion set, or
     syntax-only G10 fallback may be introduced.
8. Failed latency candidates remain unlanded. The partial V3 work may be parked only
   as historical code-first evidence after this rescope is recorded. It carries no
   approval and supplies no predecessor satisfaction.

## Supersession of AMD-002

This amendment supersedes AMD-002 point 1 only where that point makes A2C the sole
predecessor of A3. The A2C row remains present, but A3's predecessor is A2.

AMD-002 points 2 through 4 were already superseded by AMD-003 and remain non-operative.
AMD-002 point 5 remains in force: the DAG, exact-state template, and external live
ledger must retain exactly the same A2C row identifier, and the live ledger must bind
the current DAG digest. AMD-002's scope prohibitions remain binding on the deferred D6
work.

AMD-002's execution-precedence lineage `A2 → A2C → A3` is superseded. The executable
critical-path lineage is `A0 → A1 → A2 → A3 → A4 → A5 → A6`.

## Supersession of AMD-003

This amendment supersedes AMD-003 amendment points 1 through 4 as requirements on the
A2–A6 critical path. In particular:

- A2C no longer delivers an early structural slice of D6;
- A3 no longer waits for or consumes a G10 abrupt-completion verdict;
- the AMD-003 completion implementation and performance instrument are not A3–A6
  predecessor gates; and
- the AMD-003 failure-and-stop loop is closed rather than resumed.

AMD-003 remains in force as historical failure evidence and for the architectural
constraints explicitly retained above: content-free skeleton topology, demanded graph
authority, no second classifier, no fixed target ceiling, and no A3-only retained
payload. Its rejected-candidate source disposition and measurements do not transfer to
D6 acceptance. D6 must freeze its own finishable implementation lock only after the
closed code-first carrier inventory exists.

AMD-001 is unaffected and remains fully in force.

## Execution precedence

For execution, AMD-004 and the amended live split files supersede the A2C/A3 lineage
and completion-staging text in AMD-002, AMD-003, `program.md`, the pinned consolidated
master, release artifacts, `_EXTRACTION_INDEX.md`, and historical readiness reviews.

The executable lineage is:

`A0 → A1 → A2 → A3 → A4 → A5 → A6`

A2C remains a reachable terminal historical row with predecessor A2 and status
`SUPERSEDED`. The DAG, tracked template, and external live ledger each contain exactly
51 block identifiers.
