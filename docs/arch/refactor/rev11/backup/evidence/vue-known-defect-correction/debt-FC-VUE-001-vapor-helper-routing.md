# Tracked debt — remaining Vapor helper-routing gaps deferred to BV1

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition"), for a subset of a wider
finding whose other part is **REJECT**.

## What happened

BV0's relanding work (fresh corpus measurement against the BF2-reopen-3-repaired oracle) found
remaining Vapor helper-routing gaps beyond what BV0's charter-bounded fix set closes:
`withModifiers`/`withKeys`/`setDOMProp` are not routed through the official Vapor helper surface
on the affected cells. A prior finding in the same area cited `withVaporModifiers` — that specific
helper does not exist in the pinned `vue@3.6.0-rc.3` oracle (removed upstream between the rc.1 and
rc.3 lines) and is **REJECTED** as stale, not deferred.

## Ruling reference

Codex Sol xhigh scoping consult, dispatched by the Track B orchestrator during BV0's relanding
(2026-08-13), Q5 of a five-question consult also covering BF2 reopen #4 (Q2/Q4) and a new
predecessor-block requirement for assembled-module source-map composition (Q3, unresolved as of
this record — see `docs/arch/architecture-lock/ledger/program-state.toml` BV0/BF2 notes for current
status). Full transcript at the Track B worktree's scratchpad (`bv0-round2-ruling.txt`,
`bv0-round2-scope-consult.md`) as of authoring; not yet durably committed into the evidence tree —
a future block touching this debt row should re-preserve the transcript if the scratchpad path is
no longer available.

Ruling: `withVaporModifiers` — REJECT as stale (rc.3 removed it, nothing to route to).
`withModifiers`/`withKeys`/`setDOMProp` — CODEX-DEFER to BV1, this record is the required debt row.

## Durable owner

**BV1** — the Vue production-backend block (`program-dag.toml`: downstream of BF2/BF3/BV0), the
first point in the program where Verter's Vapor event-handling output is locked against the
official compiler at production scope, making it the natural owner of full Vapor helper-surface
parity rather than BV0's narrower immediate-defect-correction charter.

## Resolution gate

Before BV1's own acceptance evidence is accepted, its owned Vapor event-handling surface must
route `v-on` modifiers (`.stop`/`.prevent`/`.self`/etc.), key modifiers, and DOM-property binding
through the official `withModifiers`/`withKeys`/`setDOMProp` helpers (or the then-current pinned
oracle's equivalent surface, re-verified — do not assume rc.3's current helper names are still
current by the time BV1 lands) for every seed/official case exercising them.

## Acceptance ID

`FC-VUE-001` — "Vapor `v-on` modifier, key-modifier, and DOM-property-binding output routes through
the official Vapor helper surface matching the pinned oracle." Not satisfied by BV0 (out of its
charter's immediate-defect-correction scope). Owned by BV1.

## Current state (as of this record)

- BV0 does not claim this exit criterion. The gap is real and measured against the BF2-reopen-3
  repaired oracle (not stale pre-repair numbers).
- No `performance-gates.toml` or correctness-gate cell references this debt row yet — BV1's charter
  should name it explicitly when authored/executed.
