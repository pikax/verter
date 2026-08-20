---
ruling_id: "AT2-ACT-CLARIFICATION"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["BF3", "BA0", "AT-2 finding row"]
source_file: "MAINTAINER-ACT-AT2-CLARIFICATION.md"
summary: "Supplements MAINTAINER-ACT-AT2.md, issued after a review seat again correctly declined to infer coverage. Clarification 1: the original act covers all three hunks in BA0.md's required-RED Svelte-refusal obligation (findings-table row, Required procedure paragraph, Required exits paragraph), not just the two originally named locators. Clarification 2: reclassifying AT-2 removes it from BF3's exhaustion-exit obligation entirely (exhaustion demands evidence only for genuine failures; AT-2 is no longer classified as one) — the residual hazard is carried by the #[ignore]d test and BA0's retained ownership, and any future demonstrated reachability is a NEW finding with its own RED target."
supersedes: []
superseded_by: []
contradicts: []
notes: "Explicitly a scope clarification, not new authorization: 'Authorizes no byte beyond what is already landed; this act describes coverage, it does not request an edit.' Does not accept BF3/BA0, does not unlock B2/B3, authorizes no production guard/refusal/withhold/retraction/removal ID."
---

# Maintainer act — AT-2 amendment, scope clarification (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Supplements the AT-2 naming act of the same date. Issued after an architecture review seat
correctly refused to infer either point.

## Why this act exists

Both points below were raised because the program orchestrator UNDER-DESCRIBED the original act,
not because of any defect in the landed work. The seat declined to infer coverage in both cases.
That was the right call and is the reason this clarification exists rather than another inference.

## Clarification 1 — the third hunk in the correction-owner charter

The original act's scope bullet named two locators in `charters/BA0.md` (lines 28 and 37). The
landed edit has a THIRD hunk, in that charter's Required-exits paragraph.

**Ruled: the act covers all three hunks.** `BA0.md` states the same required-RED Svelte-refusal
obligation in three places — the findings-table row, the Required procedure paragraph, and the
Required exits paragraph. Dropping that obligation, which the original act authorizes, necessarily
edits every location stating it. The third hunk introduces no instruction the act does not already
cover and adds nothing to BA0's scope; the program orchestrator verified the diff directly before
this act was issued. Reverting it would leave the charter self-contradictory, with the rejected
obligation still live in one paragraph after being removed from the other two.

## Clarification 2 — the exhaustion exit and the reachability residual

**Ruled: reclassifying AT-2 removes it from the exhaustion obligation.**

BF3's exhaustion exit requires evidence for every GENUINE failure. The naming act reclassified AT-2
as a latent construction hazard with reachability unproven — explicitly NOT a demonstrated defect.
It therefore leaves the genuine-failure set entirely, and there is no failure left for the
exhaustion exit to demand evidence of. The charter's rule that `UNPROVEN` cannot count as
exhaustion continues to bind every row that IS a genuine failure; it does not apply to a row
correctly classified as not being one.

The residual is not discarded: it is carried by the `#[ignore]`d characterization test and by BA0's
retained ownership, which must remove the hazard as a construction property. If the hazard is ever
demonstrated reachable, that reproduction is a NEW finding with its own RED target, exactly as the
amended BA0 charter already states.

## Scope

- Authorizes no byte beyond what is already landed; this act describes coverage, it does not
  request an edit.
- Does NOT accept BF3, does NOT accept BA0, does NOT unlock B2/B3.
- Authorizes no production guard, typed refusal, withhold path, retraction, or removal ID.
