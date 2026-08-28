---
ruling_id: "AUTO-ACCEPT"
type: "maintainer-directive"
date: "2026-08-19"
date_source: "stated"
binds: ["program-wide acceptance protocol"]
source_file: "MAINTAINER-RULING-AUTO-ACCEPT.md"
summary: "Delegates the routine acceptance-recording act to the program orchestrator, conditional on ALL of: every required mandate PASS on this exact candidate per DAG class; verdicts issued by seats that actually looked at this candidate (no inherited/implicit-close verdicts); orchestrator-independently-verified identity/digests/commit hygiene/validator green; no maintainer-reserved item entangled (no amendment/rescope/gate recalibration/irreversible contract change riding along). Any BLOCKING/NOT_PROVEN mandate, anything needing ratification, an unevidenced 'pre-existing' failure claim, or known false/unverifiable evidence still comes to the maintainer."
supersedes: []
superseded_by: []
contradicts: []
notes: "Explicit: delegated is the paperwork act; NOT delegated is judgement about whether the bar was met — if in doubt, the block waits."
---

# Maintainer ruling — auto-accept on a genuine three-mandate pass (2026-08-19)

> after the review passes please accept automatically

The maintainer delegates the acceptance decision to the program orchestrator, CONDITIONAL on the
review actually passing. Acceptance is recorded without waiting for a per-block maintainer act.

## Auto-accept ONLY when ALL of these hold

1. **Every required mandate is PASS on THIS EXACT candidate**, per the block's DAG class
   (`foundational*` = all three; `subsystem` may carry `architecture_review = NOT_REQUIRED`).
2. **The verdicts were issued by seats that looked at THIS candidate.** An inherited verdict, an
   "implicit close" after unreviewed fix rounds, or a verdict upgraded by anyone other than the seat
   that issued it does NOT qualify. This is the exact failure that let an earlier block pass 3/3 twice
   while its oracle was unsound by construction.
3. **The orchestrator has independently verified** identity and digests: candidate SHA/tree reachable
   on the branch, every recorded digest recomputed and matching, commit bodies free of program
   vocabulary, no machine-path leaks, and both validator modes green.
4. **No maintainer-reserved item is entangled** — no charter/DAG amendment, formal rescope, gate
   recalibration, or irreversible public-contract change riding along.

## Still comes to the maintainer, unchanged

- Any mandate `BLOCKING` or `NOT_PROVEN`.
- Anything needing an amendment, a ratification, or a scope decision.
- A gate that did not produce a terminal PASS, or whose failures are only *claimed* pre-existing —
  the discrimination must be evidenced, not asserted.
- A block landing with known false or unverifiable evidence.

## What is delegated vs what is not

Delegated: the routine act of recording `status = ACCEPTED` / `maintainer_decision = ACCEPTED` and
unlocking successors, once the above is satisfied.
NOT delegated: judgement about whether the bar was met. Auto-accept is a shortcut through the
paperwork, never through the evidence. If in doubt, the block waits.
