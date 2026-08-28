# Maintainer act — the AT-2 amendment, scope clarified (2026-08-17)

**Maintainer:** Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
**Solicited by:** the program orchestrator, after an architecture review seat held two points open
that only the maintainer could close.

This act SUPPLEMENTS the naming act of the same date
([`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md)). The maintainer's issued
document is reproduced verbatim below, in full, as a quotation. Nothing inside the quotation has
been edited, reordered or summarized.

## The two points this act closes

Both were raised because the naming act UNDER-DESCRIBED its own coverage — a program-orchestrator
drafting error — and both were correctly refused as inferences by the review seat that met them:

1. **The third hunk.** The naming act's scope bullet enumerates two locators in
   [`../../charters/BA0.md`](../../charters/BA0.md) (lines 28 and 37). The landed edit has a THIRD
   hunk, in that charter's Required-exits paragraph, carrying the same obligation the act's
   operative clause drops. The architecture seat ruled the `RECORDED AND ESCALATED` disposition
   CORRECT and restated the requirement in its own words: *"only the maintainer can confirm
   coverage or direct reversion."*
2. **The exhaustion exit.** The architecture seat read the naming act precisely — it *"changes
   AT-2's classification and item-6 consequence, but does not amend this BF3 exit; its stated
   effect names item 6 only"* — and therefore left Required-exits sentence 1 and the "exhaust the
   retained inventory" objective `NOT-EVIDENCED`.

Neither point was a defect in the landed work, and neither was closable at track level. The seat
refusing to infer coverage is the reason this act exists rather than another inference.

## The act, verbatim

> # Maintainer act — AT-2 amendment, scope clarification (2026-08-17)
>
> Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
> Supplements the AT-2 naming act of the same date. Issued after an architecture review seat
> correctly refused to infer either point.
>
> ## Why this act exists
>
> Both points below were raised because the program orchestrator UNDER-DESCRIBED the original act,
> not because of any defect in the landed work. The seat declined to infer coverage in both cases.
> That was the right call and is the reason this clarification exists rather than another inference.
>
> ## Clarification 1 — the third hunk in the correction-owner charter
>
> The original act's scope bullet named two locators in `charters/BA0.md` (lines 28 and 37). The
> landed edit has a THIRD hunk, in that charter's Required-exits paragraph.
>
> **Ruled: the act covers all three hunks.** `BA0.md` states the same required-RED Svelte-refusal
> obligation in three places — the findings-table row, the Required procedure paragraph, and the
> Required exits paragraph. Dropping that obligation, which the original act authorizes, necessarily
> edits every location stating it. The third hunk introduces no instruction the act does not already
> cover and adds nothing to BA0's scope; the program orchestrator verified the diff directly before
> this act was issued. Reverting it would leave the charter self-contradictory, with the rejected
> obligation still live in one paragraph after being removed from the other two.
>
> ## Clarification 2 — the exhaustion exit and the reachability residual
>
> **Ruled: reclassifying AT-2 removes it from the exhaustion obligation.**
>
> BF3's exhaustion exit requires evidence for every GENUINE failure. The naming act reclassified AT-2
> as a latent construction hazard with reachability unproven — explicitly NOT a demonstrated defect.
> It therefore leaves the genuine-failure set entirely, and there is no failure left for the
> exhaustion exit to demand evidence of. The charter's rule that `UNPROVEN` cannot count as
> exhaustion continues to bind every row that IS a genuine failure; it does not apply to a row
> correctly classified as not being one.
>
> The residual is not discarded: it is carried by the `#[ignore]`d characterization test and by BA0's
> retained ownership, which must remove the hazard as a construction property. If the hazard is ever
> demonstrated reachable, that reproduction is a NEW finding with its own RED target, exactly as the
> amended BA0 charter already states.
>
> ## Scope
>
> - Authorizes no byte beyond what is already landed; this act describes coverage, it does not
>   request an edit.
> - Does NOT accept BF3, does NOT accept BA0, does NOT unlock B2/B3.
> - Authorizes no production guard, typed refusal, withhold path, retraction, or removal ID.

## The third hunk, checked against the actual bytes

The act's premise — that `BA0.md` states the same obligation in three places, so dropping it edits
all three — is checkable, and was checked rather than accepted:

```
git diff --unified=0 b75fcebc33e3a100bbfff7af62fe2edceb4fcaf0..HEAD \
  -- docs/arch/refactor/rev11/charters/BA0.md
```

reports exactly three hunks — `@@ -28 +28 @@`, `@@ -37,9 +37,14 @@`, `@@ -54,5 +59,5 @@` — and each
removes the same instruction:

| hunk | what it dropped |
|---|---|
| line 28, the findings-table row | *"After RT-1 is corrected, BA0 must prove the newly reachable Svelte-refusal batch class … add a new separately named `#[ignore]`d correct-behavior target"* |
| line 37, the Required procedure paragraph | *"Once RT-1 is corrected, prove the Svelte-refusal batch class … and prove that new target RED"* |
| line 59, the Required-exits paragraph | *"AT-2 proves the Svelte-refusal batch class after RT-1 is corrected, using a new ignored correct-behavior target"* |

The third hunk adds no instruction the act's operative clause does not already reach, and it grants
`BA0` no scope: its replacement text states that AT-2 *"carries no RED target and no Svelte-refusal
obligation"*, which is the act's own operative sentence restated as an exit. The act's reading of
its own coverage matches what is in the tree.

## How clarification 2 relates to the charter's actual words

This distinction was raised by a review seat and is recorded rather than smoothed over, because
getting it wrong would misattribute a maintainer amendment to the charter's existing text.

The act's second clarification opens: *"BF3's exhaustion exit requires evidence for every GENUINE
failure."* Read against [`../../charters/BF3.md`](../../charters/BF3.md), that is not a description
of the charter's wording. The Required-exits paragraph opens with three sentences, and only the
THIRD carries the qualifier:

> The full retained inventory has actual results. `UNPROVEN` records an open proof gap and cannot
> count as exhaustion. Every genuine failure has exact request/route/profile/products/domain
> evidence, an independently discriminating regression, root-cause classification, a named
> correction owner, and a correction acceptance/test ID; no guard or removal ID exists.

Sentences 1 and 2, and the objective's "exhaust the retained … inventory", are unconditional as
written. So the correct reading of clarification 2 is NORMATIVE, not interpretive: the maintainer
NARROWS that obligation to exclude `AT-2`, on the ground that the row is not a genuine failure. That
is an amendment to how the exit applies to one row, made by the only actor entitled to make it —
which is exactly why it could not be taken at track level, and why the seat that met the question
was right to send it here.

Nothing in the rest of the exit moves. Sentences 1 and 2 continue to bind every other row
unconditionally, and every retained product/route row in
`framework_product_surface_inventory.json` carries an actual driven result independently of this
act. What the act removes from that exit's reach is one reclassified row, and nothing else.

## What this act settles, and what it leaves alone

**Both architecture-mandate blockers are now decided by the maintainer, not by a track-level
inference.** The byte-scope question is answered as coverage (three hunks, one obligation), and the
exhaustion exit is answered as classification (a row that is not a genuine failure raises no
exhaustion obligation). The seat's refusal to infer either was correct in both cases, and the
record says so where the seat's words sit.

**No byte changes under this act.** It describes coverage; it does not authorize an edit. The
`AT-2` row in [`dispositions.md`](dispositions.md) and all three `BA0.md` hunks stand exactly as
landed. That they are byte-identical to the prior candidate is checkable directly:

```
git diff 848604ec3 -- docs/arch/refactor/rev11/evidence/BF3/dispositions.md \
                      docs/arch/refactor/rev11/charters/BA0.md
```

is empty on the commit that adds this file.

**The residual is not closed, and this record does not claim it is.** What the act rules is that
`AT-2` is not a genuine failure, so the exhaustion exit does not demand evidence of it. The
reachability question itself remains open, recorded as `UNKNOWN` in
[`dispositions.md`](dispositions.md), carried by the `#[ignore]`d characterization, and owned by
`BA0` — which must remove the hazard as a construction property regardless of whether anyone ever
reaches it. A future reproduction is a NEW finding with its own RED target. The charter's rule that
`UNPROVEN` cannot count as exhaustion is untouched and still binds every row that IS a genuine
failure.

**What this act withholds is unchanged from every prior act on this block.** BF3 is NOT accepted,
`maintainer_decision` stays `PENDING`, `BA0` is NOT accepted, B2 and B3 stay LOCKED, and no
production guard, typed refusal, withhold path, retraction or removal ID is authorized anywhere.

## Where this act's effect is recorded

- [`architecture-mandate-review.md`](architecture-mandate-review.md) — the architecture seat's two
  open points in its own unedited words, with this act's resolution recorded beneath them, and the
  re-run of that mandate on the resolved state.
- [`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md) — the conformance confirm seat's
  precise note that the naming act's `BA0` locator was under-inclusive, preserved verbatim, with
  this act's answer beneath it.
- [`maintainer-act-at2-amendment.md`](maintainer-act-at2-amendment.md) — the naming act this one
  supplements. Its "Two things this record does NOT claim" section states the first point as an
  open governance item; that item is now closed by this act, and the section is left byte-unchanged
  as the historical record of how it was raised.
- [`landing-record.md`](landing-record.md) — the block record, where both points were carried as
  acceptance blockers.
- [`dispositions.md`](dispositions.md) and [`../../charters/BA0.md`](../../charters/BA0.md) — the
  bytes the naming act authorized, deliberately left untouched by this one.
