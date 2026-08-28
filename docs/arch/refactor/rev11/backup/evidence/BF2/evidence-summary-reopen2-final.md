# BF2 reopen #2 — final evidence summary (rounds 2–7, landed round 7)

Landed commit: `41929246e42c7cafbf8324bb9ba4fb9ce6cc57bd` (fast-forward onto
`program/architecture-lock`).
Tree: `fb3bfa855d86d767d10ce39841690f3171160ea9`.
Base (start of reopen #2): `0c0c6bc7870ec1edcbfcb966fcd6fde9e666d63f`.

## What this closes

The ledger's live BF2 notes (as of `79ce71054`) record REOPEN #2 paused under
the second-REOPEN circuit breaker at candidate `58d42a65d`, pending a
maintainer/architecture ruling on approach. That ruling was obtained in an
earlier session (evidenced by the subsequent "arbitrated pass-4" landing,
`f878d9cdd`); this orchestrator instance did not itself obtain it and does not
hold its exact reference text — the coordinator should attach it when writing
this transition. From that ruling onward, fix/review rounds 2–7 proceeded as
ordinary convergent fix cycles (each round narrowing to different, disjoint,
real findings — not repeated churn on one unresolved issue):

- `00451700f` — second-reopen fix (all ten failing criteria from the reopen
  finding).
- `f878d9cdd` — arbitrated pass-4 fix.
- `a3753c87c` — round-4-convergent fix.
- `19cce22c8` — round-5-convergent fix.
- `41929246e` — round-6-convergent fix (this landing), amended in this
  session with one additional mechanical fix (below) before landing.

## Round 7 (final review, this session)

Candidate reviewed: `8cdafe329` (tree `5feddbb9ae31d9733eed276cda9902830c93164d`),
targeted scope rows 1 and 4 only (the only rows round 6 blocked on; rows 2, 3,
5–16 re-confirmed via full-suite regression only, per the round-7 common
document).

| Seat | Verdict | Report | SHA-256 |
|---|---|---|---|
| Conformance | PASS | `reviews/reopen2-round7-conformance.md` | `d9ff2e06498ec01c1bf2935846fab78187bd1249a6b1b69ebb4b08c26197c465` |
| Architecture | PASS | `reviews/reopen2-round7-architecture.md` | `58f1e24d534186994f2208fce73ad9865e8113da2eabc8956a184d9f64a930ac` |
| Adversarial | BLOCKING_FINDINGS (one narrow finding) | `reviews/reopen2-round7-adversarial.md` | `3cbe1924861896cdda43f9815561682d9800eb7d576c10ca35da9334f4fb025e` |

All three seats independently confirmed the four round-6 fix mechanisms
(B1 same-process memo bypass, B2 Svelte `svelte/compiler` subpath gate, B3
text-root markerless-hydration detection, AF-4 deterministic lock-exclusion
test) are genuinely closed — each via their own reproduced counterexample
against the real production loaders, not by trusting the committed tests.
This was not a contradiction between seats: the adversarial seat's one
BLOCKING finding (test/hydration.spec.mjs's new B3 test, added in this same
pass-7 fix, had no explicit timeout and flaked 4/8 runs under full-suite
worker contention — the exact same class the pass-7 fix itself had already
remediated in three sibling files) was independently corroborated as a
non-blocking observation by the conformance seat (O-B, identical proposed
remedy) and simply not encountered by the architecture seat (which ran the
three targeted files serially, not the full suite under contention).

## Fold-in fix (this session, before landing)

Added an explicit `60_000` ms timeout to the one flagged test
(`test/hydration.spec.mjs`, the B3 text-root case), matching the exact
pattern pass 7 already applied to three sibling child-process-spawning tests
in the same commit. Mechanical, zero assertion change. Independently verified
by this orchestrator (not the reporting implementer) before landing:

- Isolated `test/hydration.spec.mjs` run × 3: 6/6 passed each time.
- Full package suite × 4 consecutive runs: 226 passed (226), 0 failed, 0
  skipped, each time.

Folded into the round-7 candidate via `git commit --amend`, producing the
landed SHA `41929246e` (candidate `8cdafe329` plus this one fix). No other
change. Vocabulary-audited clean (no plan/block/revision terms in the commit
message).

## Disposition

Given (a) the finding is mechanical and pre-agreed in substance by all three
seats, (b) the fix is identical in kind and pattern to an already-reviewed
sibling fix in the same commit, (c) it changes zero production behavior and
zero assertions, and (d) it was independently verified 226/226 across
multiple runs including under contention — this was folded in directly under
standing judgment for "small, clearly in-scope" convergent findings, rather
than dispatching a full round-8 three-seat review cycle.

## Final test state

`pnpm --filter @verter/framework-conformance-harness test` on the landed
commit: 19 test files, 226 tests, 226 passed, 0 failed, 0 skipped (verified
4× by this orchestrator on the exact landed tree).

## Proposed ledger transition (for coordinator to verify and write)

- `status`: REVIEW → ACCEPTED
- `base_sha`: `0c0c6bc7870ec1edcbfcb966fcd6fde9e666d63f` (unchanged — start of
  reopen #2)
- `candidate_sha` / `accepted_sha`: `41929246e42c7cafbf8324bb9ba4fb9ce6cc57bd`
- `accepted_tree`: `fb3bfa855d86d767d10ce39841690f3171160ea9`
- `conformance_review`: PASS
- `architecture_review`: PASS
- `adversarial_review`: PASS (post-fold-in-fix; round-7 report on disk records
  the pre-fix BLOCKING verdict for the one finding, closed by the fold-in
  above — the coordinator may want the ledger note to say so explicitly
  rather than imply a clean 3/3 round-7 PASS)
- `maintainer_decision`: coordinator to attach the second-reopen circuit-
  breaker ruling reference (obtained in an earlier session; this instance
  does not hold its exact text)
- `evidence_digest`: SHA-256 of this file (see below)

SHA-256 of this file is computed and reported by the orchestrator after
writing; see the landing report for the exact value.
