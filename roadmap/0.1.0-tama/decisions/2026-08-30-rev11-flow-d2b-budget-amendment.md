# D2B budget amendment (rev11.flow)

- Status: accepted
- Date: 2026-08-30
- Amends: `charters/rev11-flow/D2B.md` budget section
- Scope: D2B only; no other node's budgets change

## Context

D2B's charter carried the standard 800 production LOC / 8 files target and the 1,500 LOC / 12 files mandatory-rescope trigger. The codex D2 scope ruling already established that a sound atomic cutover touches at least ten production files, and its rescope ruling on the landed candidate was explicit: `RESCOPE_REQUIRED — BUDGET_AMENDMENT`, not a further DAG split — the cutover is atomic by nature (a partial cutover would preserve one of the distributed admission channels it exists to retire), the work was complete and green, and splitting after the fact would manufacture artificial boundaries inside one indivisible semantic change.

The then-current D2B candidate as measured at this amendment's drafting (HISTORICAL measurement — see the correction below; commits `8db253e6b`, `b9f746f48`, `402bed56a`, `3cceb4dce`, `35d1bbd55`) totaled 20 production files in one crate (`verter_session`), +3,262/−619 production LOC. The bulk is the evaluator wiring the cutover exists for: per-demand install/prepare, evaluator-witnessed discharge evidence, real convergence, the finalizer adapter, proof-typed SCC members, the poisoning rails, and the control-call certification closure. Roughly 160 LOC of the gross count is `cfg(test|test-support)`-gated fault injection inside production files.

## Decision

Amend D2B's budget to the landed reality: the 800/8 target and the 1,500/12 rescope trigger do not apply to D2B. The binding constraints that remain in force for the candidate are the correctness budget (zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing) and the review profile (public-3), both satisfied by the review rounds on record. [STALE — see the correction below: later content invalidated those review rounds and D2B-AC2 is presently OPEN. The 800/8 target and 1,500/12 rescope trigger not applying to D2B remains the standing decision; the "both satisfied" clause does not.]

## Consequences

- D2B's ledger row stands without a rescope split.
- Successor nodes (D3R/D3I/D3P/D3C per `decisions/2026-08-30-rev11-flow-d3-split.md`, then D4–D8) keep their standard budgets; the D3 split is the evidence that over-budget work continues to be split rather than amortized.
- This amendment changes no other node's charter or budget. D2B's charter and DAG machine fields record the accepted 3,262-LOC/20-file/one-crate comparison footprint without creating a numeric rescope trigger.

## Correction (architect review, 2026-09-01)

Recorded in place rather than silently amended, per this trail's standing
retraction convention.

- **Measurement label.** The "landed D2B candidate" paragraph under Context above
  describes the five-commit/20-file shape captured when this amendment was
  drafted, not D2B's actual final landed candidate. D2B's landed range is
  `9cc859c8c..acb5b0b67` (49 commits — substantially larger; the canonical
  type-algebra predecessor work required by
  `decisions/2026-08-31-canonical-type-algebra-predecessor.md` grew the candidate
  after this amendment was accepted). The variance conclusion — that the original
  800/8 target and 1,500/12 rescope trigger do not fit D2B's atomic-cutover shape
  — remains valid; only the specific figure is relabeled "then-current candidate"
  rather than "landed candidate."
- **Review-status claim retracted.** The Decision section's statement that the
  correctness budget and the `public-3` review profile were "both satisfied by
  the review rounds on record" is STALE and is retracted in place rather than
  deleted. The canonical-type-algebra predecessor ruling
  (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`) found four
  blocking defects (A–D) against the reviewed candidate, added TA1A, TA1B and TA2
  as explicit D2B predecessors, and required D2B's review profile to re-run —
  "the content change invalidates every current verdict." D2B-AC2 (positive
  contract — exact identity) is presently OPEN; this amendment's "satisfied"
  language does not stand as a completion claim for AC2 and must not be read as
  one.
- The budget conclusion itself — that the 800/8 target and the 1,500/12 rescope
  trigger do not apply to D2B — is UNCHANGED by this correction; see
  `charters/rev11-flow/D2B.md` → "Budgets and rescope" and
  `authority/dag/rev11-flow.toml`, which record the amendment's accepted
  3,262-LOC/20-file/one-crate comparison footprint without making it a numeric
  rescope trigger.
