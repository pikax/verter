# D2B budget amendment (rev11.flow)

- Status: accepted
- Date: 2026-08-30
- Amends: `charters/rev11-flow/D2B.md` budget section
- Scope: D2B only; no other node's budgets change

## Context

D2B's charter carried the standard 800 production LOC / 8 files target and the 1,500 LOC / 12 files mandatory-rescope trigger. The codex D2 scope ruling already established that a sound atomic cutover touches at least ten production files, and its rescope ruling on the landed candidate was explicit: `RESCOPE_REQUIRED — BUDGET_AMENDMENT`, not a further DAG split — the cutover is atomic by nature (a partial cutover would preserve one of the distributed admission channels it exists to retire), the work was complete and green, and splitting after the fact would manufacture artificial boundaries inside one indivisible semantic change.

The landed D2B candidate (commits `8db253e6b`, `b9f746f48`, `402bed56a`, `3cceb4dce`, `35d1bbd55`) totals 20 production files in one crate (`verter_session`), +3,262/−619 production LOC. The bulk is the evaluator wiring the cutover exists for: per-demand install/prepare, evaluator-witnessed discharge evidence, real convergence, the finalizer adapter, proof-typed SCC members, the poisoning rails, and the control-call certification closure. Roughly 160 LOC of the gross count is `cfg(test|test-support)`-gated fault injection inside production files.

## Decision

Amend D2B's budget to the landed reality: the 800/8 target and the 1,500/12 rescope trigger do not apply to D2B. The binding constraints that remain in force for the candidate are the correctness budget (zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing) and the review profile (public-3), both satisfied by the review rounds on record.

## Consequences

- D2B's ledger row stands without a rescope split.
- Successor nodes (D3R/D3I/D3P/D3C per `decisions/2026-08-30-rev11-flow-d3-split.md`, then D4–D8) keep their standard budgets; the D3 split is the evidence that over-budget work continues to be split rather than amortized.
- This amendment changes no other charter, budget, or DAG field.
