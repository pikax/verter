---
ruling_id: "BV2-B5-J1-RATIFICATION"
type: "maintainer-ruling"
date: "2026-08-22"
date_source: "stated"
binds: ["BV2", "B5", "J1", "review-mandate protocol"]
summary: "Accepts BV2; retroactively authorises B5's landing and permits J1 to land as slices; accepts the single independent review lane in place of three named mandates for these blocks only. Records the orchestration failures that made the ruling necessary and the controls adopted so they do not recur."
supersedes: []
superseded_by: []
contradicts: []
---

# Maintainer ruling — BV2, B5 and J1

**Status:** ADOPTED by the maintainer, 2026-08-22.

## Why this ruling exists

An independent audit of the ledger and of the day's orchestration found two
critical protocol violations and several record defects. This ruling disposes of
them. It does not pretend they did not happen — the failures are recorded below
because the controls only make sense against them.

## 1. BV2 — ACCEPTED

BV2's predecessors BV1 and BS1 are both `ACCEPTED`, so BV2 was legitimately
authorised; its acceptance was simply never recorded. Its work is on trunk in two
parts: the recorded candidate `b64a440a9` (landed 2026-08-21) and
`979123ef4` (landed 2026-08-22, the TSC expose bundle failing closed on any
corrupt identity).

Noted as part of the accepted state, not hidden: `979123ef4` introduced a
regression — a degraded class-method inference escalated to the whole props
surface — which was found by review, bisect-confirmed against its parent, and
fixed in `3f663584e`. The accepted behaviour is the post-fix behaviour.

## 2. B5 — retroactively AUTHORISED and accepted

B5 was dispatched and landed (`c68fe61e3`) while its charter still read
"PROPOSED amendment / LOCKED" and while its DAG predecessor BV2 was not accepted.
That was an orchestration error: the readiness check used "predecessor has an
evidence directory" as a proxy for ACCEPTED, and that proxy is wrong.

With BV2 accepted above, B5's predecessor requirement (`BV1`, `BS1`, `BV2`) is
now genuinely satisfied. Its charter is moved from LOCKED to authorised and its
landing is ratified. The work itself was reviewed to a LAND verdict across three
rounds and carries a byte-identity differential (76/76) proving the accepted Vue
and Svelte packs come through the direct route unchanged.

## 3. J1 — partial landing PERMITTED

J1 is one foundational block in the DAG, and `contracts/stacked-prs.md` §8 bars a
stack from silently splitting a program acceptance unit, §43 bars an atomic
unit's private layers from landing independently. Two J1 slices were nonetheless
landed (`eadec2dc0`, `120eede71`) under a 4-slice split the orchestrator ratified
on its own authority. It had no such authority.

The split is now ratified explicitly: **J1 may land as slices**, with the
remaining slices (`block/css-cutover`, `block/svelte-css-grammar`,
`block/css-closing-items`) landing under the same permission. J1's acceptance is
complete only when every one of its 41 acceptance IDs is covered; the landed
slices do not constitute acceptance of the block.

This permission is specific to J1. It is not a general licence to split an
acceptance unit.

## 4. Review mandates — single lane accepted for these blocks only

The ledger requires `conformance_review`, `architecture_review` and
`adversarial_review` as three separately-named mandates. These blocks received
one independent codex review lane each, iterated to clean (3 to 9 rounds
depending on the block), plus a retroactive full-diff review that found three
further defects, now dispatched as fixes.

That single lane is accepted as satisfying the mandate requirement **for BV2, B5,
J1 and CM1 only**. It is not a protocol change: future blocks run all three.

## 5. What went wrong, recorded

- readiness was inferred from evidence-directory presence rather than `ACCEPTED`
  status, which is how a LOCKED block with an unaccepted predecessor was
  dispatched
- an acceptance unit was split on the orchestrator's own authority
- landings were recorded retrospectively rather than in the same change, so a
  rehearsal pin went stale unnoticed and eleven of fifteen landings still have no
  ledger disposition
- the landing record overstated: it said "fourteen blocks" for a fifteen-commit
  range, included a commit outside that range, asserted a green gate with no
  durable receipt, and claimed no charter digests existed when BV2, CM1 and J1
  already had them

## 6. Controls adopted

1. **Readiness means `status = "ACCEPTED"` in the ledger.** Not an evidence
   directory, not a green branch, not a finished-looking worktree.
2. **A block's ledger row is updated in the same change that lands it.** No
   retrospective reconciliation.
3. **Splitting an acceptance unit requires a ruling before the first slice
   lands**, not after.
4. **Every landing gets a ledger disposition** — an owning block, an amendment,
   or an explicit non-program note. Trunk deltas with no program authority are
   themselves a defect.
5. **The landing record states only what has a receipt.** A gate verdict without
   a durable log is not a green gate.
