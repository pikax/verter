# Maintainer act — the AT-2 amendment, named explicitly (2026-08-17)

**Maintainer:** Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
**Solicited by:** the program orchestrator, after a review seat blocked on the authority chain
behind the `AT-2` amendment already present in the tree.

The maintainer's issued document is reproduced verbatim below, in full, as a quotation. Nothing
inside the quotation has been edited, reordered or summarized.

## The defect being cured

The `AT-2` row in [`dispositions.md`](dispositions.md) and the matching obligation lines in
[`../../charters/BA0.md`](../../charters/BA0.md) were amended under the maintainer's GENERAL
standing ruling on bug handling and the type waiver
([`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md)).
That ruling never names `AT-2`; applying it to a specific ratified findings row was the program
orchestrator's reading, and that file said so in its own words. A review seat held the reading
insufficient and asked for an explicit act naming `AT-2` or a revert of both byte locations. The
seat was right: an earlier independent consult had already ruled that no track-level actor may
change the finding, class, owner or gating test of a ratified row
([`at2-disposition-ruling.md`](at2-disposition-ruling.md)), and acting on an unnamed authority is
the same governance defect that blocked this block once already. The maintainer was asked directly
and issued the act below.

## The act, verbatim

> # Maintainer act — AT-2 amendment, named explicitly (2026-08-17)
>
> Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
>
> ## Why this act exists
>
> The AT-2 row was amended in the tree by inferring authority from the maintainer's GENERAL
> bugs-and-types standing ruling of 2026-08-17. A review seat blocked that inference: the general
> ruling never NAMES AT-2, and a general act does not authorize a change to a specific ratified
> findings row. The seat was correct — acting on an unnamed authority is the same governance defect
> that blocked this block once already. The maintainer was asked directly and issued the act below.
>
> ## The act
>
> > Reject AT-2's claim that a reachable batch entry publishes a product beside a genuine typed
> > refusal; reclassify AT-2 as a latent HostBacked construction hazard with reachability unproven;
> > retain the DEFER to BA0; carry it as an `#[ignore]`d characterization test; and drop the
> > required-RED Svelte-refusal atomicity target.
>
> ## Scope
>
> - Authorizes exactly the bytes already in the tree: the AT-2 row in
>   `evidence/BF3/dispositions.md` and `charters/BA0.md` lines 28 and 37.
> - The rest of the ratified findings table is byte-unchanged and stays that way.
> - Authorizes NO production guard, typed refusal, withhold path, retraction, or removal ID.
> - Does NOT accept BF3, does NOT accept BA0, and does NOT unlock B2/B3.
>
> ## Evidence this act rests on
>
> All nine `CompileBatchEntry` construction sites enumerated in `host_compile.rs`: eight are atomic by
> hardcoded literal; the typed refusal (`RuntimeSurfaceRefused`) lands on an atomic arm publishing no
> product; the single non-atomic site (the HostBacked `Ok(response)` path) has no demonstrated
> reachable input. A later plant confirmed the hazard is real as a CONSTRUCTION property — injecting an
> error-severity diagnostic into the success response downstream of the routing gate produces an entry
> carrying a 480-byte product beside the error — while reachability through a real request remains
> unproven and was probed without reproduction. The previously-cited gating test drove a different
> failure class entirely (duplicate-canonical conflict), which publishes nothing.
>
> ## Effect
>
> The seat's authority objection is discharged. Charter procedure item 6 is no longer blocked on AT-2.
> One acceptance blocker remains and is unrelated: `architecture_review` is NOT_PROVEN because only two
> review seats were commissioned in the closing round — an orchestrator scoping error. BF3 is
> foundational class and requires all three mandates PASS, so a full architecture seat is still owed.

## What this act settles, and what it leaves alone

The AUTHORITY question on `AT-2` is closed. The amendment now rests on an act that names the row,
states the four clauses it authorizes, and bounds itself to the two byte locations already present
in the tree. Those bytes are **unchanged by this record** — the act ratifies what is there; it does
not license a further edit. That they are byte-identical to what the act authorizes is checkable
directly:

```
git diff 9104e0be7 -- docs/arch/refactor/rev11/evidence/BF3/dispositions.md \
                      docs/arch/refactor/rev11/charters/BA0.md
```

is empty on the commit that adds this file.

What the act WITHHOLDS is unchanged from every prior act on this block: BF3 is NOT accepted,
`maintainer_decision` stays `PENDING`, `BA0` is NOT accepted, B2 and B3 stay LOCKED, and no
production guard, typed refusal, withhold path, retraction or removal ID is authorized anywhere.

The act also names, in its own words, the one acceptance blocker that survives it: the closing round
commissioned two review seats where a foundational-class block requires three, so a full
architecture mandate is still owed. That is an orchestrator scoping error, not a seat finding, and
this act does not cure it.

## Two things this record does NOT claim

**1. The act's `BA0.md` locator is under-inclusive, and that is not fixed here.** The scope bullet
names `charters/BA0.md` lines 28 and 37. The block's actual `BA0.md` edit has three hunks — the row
at 28, the AT-2 procedure paragraph at 37, and a third at the Required-exits paragraph:

```
git diff --unified=0 b75fcebc33e3a100bbfff7af62fe2edceb4fcaf0..HEAD \
  -- docs/arch/refactor/rev11/charters/BA0.md
```

reports `@@ -28 +28 @@`, `@@ -37,9 +37,14 @@` and `@@ -54,5 +59,5 @@`. The third hunk carries the
same instruction the act's operative clause drops — that AT-2 prove the Svelte-refusal batch class
with a RED target once RT-1 is corrected — so it falls within the act's OPERATIVE sentence while
sitting outside its locator enumeration. The enumeration came from
[`at2-deviation-memo.md`](at2-deviation-memo.md), which named the same two locations and missed the
third.

Nothing in this record widens the act to cover it. A track-level actor may not widen a maintainer
act any more than it may amend a ratified row — that inference is the exact defect this act was
issued to cure. The bytes stand as landed and byte-unchanged; the maintainer is asked to confirm the
act reaches the third hunk, or to direct that it be reverted to its ratified text. Until then this
is an open governance item, and acceptance should not be granted on the assumption it is closed.

**2. Two prose statements elsewhere predate this act and were deliberately left byte-unchanged.**
[`dispositions.md`](dispositions.md)'s observation note still says the authority objection is
"recorded rather than answered", and its closing section still says the block made "no amendment".
Both were written before this act existed and both are superseded by it. They are NOT edited here,
because this act authorizes the bytes already present and its `dispositions.md` scope is the `AT-2`
row alone; editing the surrounding prose under an act that does not name it would repeat the
inference the act exists to stop. The correct reading order is: this act, then the row, then that
note as historical context. Reconciling the note's wording is a separate authorized edit, not
something to take here.

## Where this act's effect is recorded

- [`at2-deviation-memo.md`](at2-deviation-memo.md) — the memo that asked for this act, discharged
  by it.
- [`exhaustion-closure-reviews.md`](exhaustion-closure-reviews.md) — the blocking seat's report,
  preserved verbatim, with the resolution recorded beneath it.
- [`maintainer-standing-ruling-bugs-and-types.md`](maintainer-standing-ruling-bugs-and-types.md) —
  the general ruling whose application to this row this act supersedes as the authority. That
  ruling remains valid for exactly what its own text says.
- [`dispositions.md`](dispositions.md) and [`../../charters/BA0.md`](../../charters/BA0.md) — the
  authorized bytes themselves, deliberately left untouched.
