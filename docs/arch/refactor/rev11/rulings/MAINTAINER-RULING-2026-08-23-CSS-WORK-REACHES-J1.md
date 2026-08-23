---
ruling_id: "CSS-WORK-REACHES-J1"
type: "maintainer-ruling"
date: "2026-08-23"
date_source: "stated"
binds: ["J1"]
source_file: "MAINTAINER-RULING-2026-08-23-CSS-WORK-REACHES-J1.md"
summary: "Records the maintainer's decision on how remaining CSS work reaches J1: no new DAG blocks; the slices integrate into branch block/j1-integration, which lands on the protected trunk as the single J1 block carrying J1's 41 acceptance IDs unchanged. Selects the CSS allocation-ownership ruling's own stated fallback (private layers combine into one final J1 candidate) rather than overriding either that ruling or MAINTAINER-RULING-2026-08-22-BV2-B5-J1."
supersedes: []
superseded_by: []
contradicts: []
notes: "Creates no DAG block and no ledger block row. Does not amend J1's charter. Does not move any of J1's 41 acceptance IDs onto a slice. A slice landing into the integration branch is not an acceptance event."
---

# Maintainer ruling — how CSS work reaches J1

**Status:** RATIFIED by the maintainer, 2026-08-23.

## The apparent conflict

Two ratified rulings appeared to conflict on how remaining CSS work reaches J1.

`ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP.md` §"Landing
sequence" step 1:

> 1. Record the DAG/charter amendment creating the Svelte convergence block and the
>    two allocation predecessors. Without it, all three worktrees remain private
>    layers and must combine into one final J1 candidate.

`MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §3:

> J1 is one foundational block in the DAG, and `contracts/stacked-prs.md` §8 bars a
> stack from silently splitting a program acceptance unit, §43 bars an atomic
> unit's private layers from landing independently. Two J1 slices were nonetheless
> landed (`eadec2dc0`, `120eede71`) under a 4-slice split the orchestrator ratified
> on its own authority. It had no such authority.
>
> The split is now ratified explicitly: **J1 may land as slices**, with the
> remaining slices (`block/css-cutover`, `block/svelte-css-grammar`,
> `block/css-closing-items`) landing under the same permission. J1's acceptance is
> complete only when every one of its 41 acceptance IDs is covered; the landed
> slices do not constitute acceptance of the block.
>
> This permission is specific to J1. It is not a general licence to split an
> acceptance unit.

The first asked for a DAG/charter amendment creating a Svelte-convergence
block plus two allocation predecessors. The second makes J1 **one**
acceptance unit of 41 acceptance IDs.

## Decision

The maintainer takes the second path. **No new DAG blocks.** The slices
integrate into this branch, `block/j1-integration`, and that branch lands on
the protected trunk as the single J1 block, carrying J1's 41 acceptance IDs
unchanged.

This is the CSS ruling's own stated fallback — "all three worktrees remain
private layers and must combine into one final J1 candidate" — so this
decision needs **no amendment to the DAG or to J1's charter, and no new
ledger rows.** It does not override either ruling's substance.

## Integration branch

The integration branch is `block/j1-integration`. The slices merge there,
not onto the protected branch.

## Forced slice order

Preserved verbatim from
`ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP.md` §"Landing
sequence" steps 2–6. Under the fallback this decision selects, each step
lands into `block/j1-integration` (the one final J1 candidate), not onto the
protected branch.

> 2. Land the **Svelte grammar convergence** block first: commit its outstanding
>    work, delete the superseded Svelte grammar in the same block, prove byte-exact
>    parity, restack on current protected trunk.
> 3. Land the **`verter_css_syntax` allocation** predecessor on the final
>    shared-parser shape, including any parser-correctness changes that belong there.
> 4. Land the **`verter_compiler` arena-lifecycle** predecessor; rerun both count
>    and requested-byte attribution.
> 5. Do **not** land `block/css-closing-items`. Use it as a donor.
> 6. Reconstruct the **cutover** on the cumulative trunk, absorb the remaining
>    closing items, enable and pass all eleven canaries — or complete a valid formal
>    recalibration — then switch every public route while deleting `lightningcss`
>    and the legacy `css/` tree in one squashed commit.

Named branches, in that order:

1. Svelte grammar convergence — `block/svelte-css-grammar`
2. `verter_css_syntax` allocation predecessor — `block/css-parser-allocation`
3. `verter_compiler` arena-lifecycle predecessor — `block/css-arena-lifecycle`
4. the cutover reconstructed on the cumulative result — `css-cutover-squashed`

## Donor

`block/css-closing-items` is a **DONOR ONLY and is never landed.** Its
worktree is already removed; the branch is retained at `afc2ddbd3` because
its remaining A9/A12/A13/A14 hunks are not yet proven rehomed and must be
transplanted into the cutover.

## Acceptance remains one unit

J1 acceptance remains **ONE** unit of 41 acceptance IDs. No acceptance ID
moves to a slice. A slice landing into the integration branch is not an
acceptance event.
