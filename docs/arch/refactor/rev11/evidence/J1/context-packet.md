# J1 — context packet

Written **before** the dispatch it governs, which is the only thing that makes it a context
packet rather than a reconstruction.

Base `30ca1c5573248ad37f983d430da171cd8cd6dbb0` (the integration branch's base). Integration
branch `block/j1-integration`, currently `374b375c8`. Predecessors A4 and A6, both ACCEPTED.
Charter `docs/arch/refactor/rev11/charters/J1.md`, `**Status:** RATIFIED 2026-08-21`, digest
`ad982473306df5a093526968de0d1c42fbe4f8f514372a35bb5d96e55791fa17`, equal to the ledger row's
`charter_digest`.

## What this packet covers, and what it does not

**Covers** the remaining J1 dispatch: forced-order slices 2, 3 and 4 into `block/j1-integration`,
the `block/css-closing-items` donor transplant, and the eventual 41-acceptance-ID package.

**Does not cover** the two slices already landed on trunk — `eadec2dc0` (converge the CSS readers
on one shared parse) and `120eede71` (bring `style_planner` to parity with the legacy CSS route).
Those were dispatched before this packet existed and nothing here is claimed to have governed
them. Nor does it cover slice 1 (Svelte grammar convergence), already staged on the integration
branch as a rebase of `block/svelte-css-grammar`.

Stating that boundary is the point. A packet that silently claimed the landed work would be the
backdated artifact this document exists to avoid.

## Standing architecture this dispatch may not route around

Verter owns its CSS engine. `lightningcss` is to be **removed**; `verter_css_syntax` is the single
CSS authority. A gap in it is **implemented**, never worked around, and no slice may introduce a
second CSS parse path or defer a gap to a consumer.

`MAINTAINER-RULING-2026-08-23-CSS-WORK-REACHES-J1.md`: remaining CSS work integrates into
`block/j1-integration`, which lands as the single J1 block. No new DAG blocks; the 41 acceptance
IDs are unchanged. `MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §3: J1 is one acceptance unit;
acceptance is not recorded until every ID is covered, and the single independent review lane
(ratified for BV2/B5/J1/CM1 only) applies once acceptance review begins.

## Forced slice order

From the `CSS-ALLOCATION-OWNERSHIP-2026-08-23` landing sequence, recorded on the ledger row. The
order is forced; a later slice may not be dispatched before its predecessor is in.

1. Svelte grammar convergence (`block/svelte-css-grammar`) — **staged** on the integration branch.
2. `verter_css_syntax` allocation predecessor (`block/css-parser-allocation`, `de2021491`,
   17 commits, base `30ca1c557`) — **the currently dispatched slice**.
3. `verter_compiler` arena-lifecycle predecessor (`block/css-arena-lifecycle`, `e40c7410f`,
   6 commits) — not yet dispatched.
4. Cutover reconstructed on the cumulative result (`css-cutover-squashed`, `b78e62167`,
   5 commits) — not yet dispatched; goes last, on the cumulative result of 1–3.

`block/css-closing-items` (`afc2ddbd3`) is a **donor only** and is never landed. Its worktree is
already removed. The A9/A12/A13/A14 hunks are **not yet proven rehomed** and must be transplanted
into the cutover at slice 4. That transplant is the unquantified risk in the remaining work and is
deliberately measured rather than estimated.

A slice landing into the integration branch is **not** an acceptance event.

## Slice 2 — what is dispatched now

`block/css-parser-allocation` integrates into `block/j1-integration`: allocation attribution by
phase in `StyleSyntaxIr` construction, removal of per-rule selector-buffer pre-sizing and its
quadratic signature, duplicate selector-clone removal, moving a functional pseudo's argument list
into its component, indented-dialect statement classification aligned with the shared parser,
opaque indented unknown at-rule bodies, declaration block-value decided by the shared value shape,
and selector-window diagnostics pinned into the style IR.

Its final commit (`de2021491`) **corrects the parse-allocation record and escalates a residual**.
That escalation is inherited, not introduced by this integration, and it is to be read and
dispositioned explicitly rather than carried silently.

Both this branch and the integration branch share base `30ca1c557`, so the integration is expected
to be mechanical. It is not to be forced: a non-mechanical conflict stops and reports.

## Evidence state at dispatch

There is **no** `results/J1/` directory and no sha-bound review evidence for J1 of any kind. The
CSS-related files under `~/.claude/briefs/rev11/verify/` are raw traces, several 1.3–1.9 MB, with
no receipt lines. **None may be cited as a verdict.** Every acceptance claim for J1 is built fresh
and bound to a sha through `check-results.mjs`.

## Known forward obligation

The 41 acceptance IDs have no coverage map anywhere under `evidence/J1/`. Building that map is
part of the acceptance package, not of any integration slice, and it is not started.
