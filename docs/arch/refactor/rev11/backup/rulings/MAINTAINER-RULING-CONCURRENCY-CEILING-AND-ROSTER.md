---
ruling_id: "CONCURRENCY-CEILING-AND-ROSTER"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["program-wide concurrency policy"]
source_file: "MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md"
summary: "Concurrency authorised in principle, conditional on a mechanically testable property (git merge-tree --write-tree rehearsal, no merge-conflict risk). Ceiling: up to 5 concurrent blocks/trains on claude-max, superseding an earlier 2-block cap. Beyond 5, additional claude-max orchestrators may dispatch, but with grok as the implementer instead of claude-max. Notes the ratified validator still fails closed at one IN_PROGRESS block and needs a reviewed change plus the fence/rehearsal/review-binding machinery before the ceiling is actually usable; flags one live defect (unbound review verdicts) as already a false-green in serial mode, worse under concurrency; and poses an open design question on the restack cascade cost at N>2, with a candidate byte-identity-equivalence answer under evaluation."
supersedes:
  - document: "the orchestrator's own P2 proposal (not part of this corpus)"
    claim: "A 2-block concurrent-train cap."
superseded_by: []
contradicts: []
notes: "This ruling's numeric ceiling supersedes the earlier ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md's implicit low certification-concurrency recommendation (see that document's superseded_by field) — the underlying quadratic-gate-cost analysis in that document is not contradicted, only the resulting numeric policy is superseded by direct maintainer ratification."
---

# Maintainer ruling — concurrency ceiling 5, and the implementer roster beyond it

**Date:** 2026-08-20.

> I'm keen to do block trains/block concurrently if there's no risk of merge conflicts and cherry-pick
> is clean or very straight forward
>
> you may use more than 2 if needed up to 5 using claude-max, if you need more you may request dispatch
> more claude-max train/block orchestrators concurrently but change the implementer to be grok

## Normalised

1. **Concurrency is authorised in principle**, conditional on a real property, not a promise: no merge
   conflict risk, and a clean or very straightforward cherry-pick. That condition is now MECHANICALLY
   TESTABLE — `git merge-tree --write-tree` (git 2.55) rehearses a restack without touching a worktree,
   so "the cherry-pick is clean" is verified in advance rather than asserted.
2. **Ceiling: up to 5 concurrent blocks/trains on `claude-max`.** This supersedes the 2-block cap in the
   orchestrator's own P2 proposal. It does NOT supersede the safety preconditions — the ceiling is a
   permission, not an instruction to run 5 regardless of whether the pairwise conditions hold.
3. **Beyond 5**, more `claude-max` train/block orchestrators may be dispatched concurrently, but those
   additional trains use **grok as the implementer** rather than claude-max. Orchestration stays
   claude-max; only the implementer seat changes.
4. Standing constraints unchanged: codex never orchestrates; review seats stay codex/grok plus a
   claude-max adversarial subagent; the maintainer alone accepts blocks.

## What still gates it (orchestrator's obligation, not a maintainer condition)

The ratified validator still fails closed at ONE `IN_PROGRESS` block
(`scripts/validate-program-state.mjs:794-805`), and its own comment says a parallel regime "must relax
this check under review, not ad hoc". So raising the ceiling requires a REVIEWED validator change plus
the fence/rehearsal/review-binding machinery — the ruling authorises the destination, it does not itself
relax the check.

**One live defect must be fixed first, independent of concurrency:** review verdicts are bare strings
with nothing binding them to the candidate they were issued against, while the validator's own message
claims a foundational block needs all three mandates "PASS on one exact candidate SHA/tree". Under
concurrency a restack would silently inherit stale verdicts. This is already a false-green in SERIAL
mode.

## Open design question at N > 2 — the restack cascade

With a fixed landing order, the Nth block restacks N-1 times, and if every restack invalidates review,
block 5 pays four full re-attestations. That cost likely dominates the throughput gain. Candidate
answer under evaluation: reuse the program's EXISTING `landing_equivalence_digest` machinery — if a
restack is conflict-free and the block's own diff over its declared fence is byte-identical to the
reviewed candidate's diff over that fence, the review's subject matter is unchanged, so the verdict may
carry with a recorded, mechanically computed equivalence proof instead of a full re-review.
