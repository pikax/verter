# Tracked debt — BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE stays open

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition").

## What happened

BF2 froze the `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` cell in
`performance-gates.toml` from a 10-run measurement of its own,
just-built `packages/framework-conformance-harness/bin/generate-goldens.mjs`.
This is the forbidden pattern named in `docs/arch/refactor/rev11/governance.md`'s
Gate authority sentence: "Candidate measurements cannot be used to choose
their own pass criteria." A Codex Sol xhigh architecture-parallelism
consult, dispatched during BF2's reopen review, independently re-examined
the cell and confirmed the violation.

## Ruling reference

- Consult: Codex Sol, `model_reasoning_effort=xhigh`, run during the BF2
  reopen (2026-08-12). Full ruling text preserved at
  `.agent-run/BF2-CONSULT-RESULT.md` in the orchestrating session's
  worktree at time of writing; essential content reproduced below since
  that path is ephemeral.
- Maintainer decision: **FALLBACK** — do not pursue the consult's proposed
  bootstrap-gate protocol at this time. Withhold the freeze. Restore the
  row to explicitly open/deferred in `performance-gates.toml` and
  `docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`.
  BF2 must not claim this performance exit passed.

### Consult ruling summary (RULING: (c))

Neither (a) "freeze from BF2's own measurement under a margin" nor (b)
"cell cannot be frozen until a different implementation exists" — a fixed
multiplier over an unvetted first-implementation measurement still lets an
arbitrarily slow candidate manufacture a passing gate, and Revision 11
defines review independence by role/context/authority rather than by
requiring a second implementation.

Instead: gate AUTHORITY must be separated from implementation. The
consult's recommended (but NOT adopted — see maintainer decision above)
7-step bootstrap protocol was:

1. Maintainer ratifies the protocol itself as a narrow Implementation Lock
   Record amendment before any fresh run; BF2's existing 10-run session and
   derived thresholds are permanently invalid/superseded, retained only for
   audit.
2. Freeze the exact measurement subject (candidate SHA/tree, blobs,
   package versions, command, runner class, sandbox, control-drift bound).
3. Maintainer appoints an independent gate authority who never touched
   BF2's invalid numbers and has `NOT PROVEN` authority.
4. That authority commits a digest-addressed pre-measure registration
   (absolute wall/RSS budgets justified as product/CI budgets, NOT a
   multiple of BF2's number; 30 cold runs per
   `[statistics].short_min_samples`; relative-noise formula fixed as
   `max(3.0000%, 2 * population coefficient of variation)`) — ratified
   BEFORE the first calibration run, no tuning after.
5. A neutral runner (not the implementer) executes exactly the registered
   30-run calibration session; mechanical-only derivation of final
   numbers.
6. A disjoint post-freeze ABBA holdout session (30 invocations/arm)
   supplies the actual pass/fail evidence — not the calibration session
   itself.
7. Fresh conformance/architecture/adversarial review on one exact
   candidate with this as digest-addressed evidence.

The maintainer declined to invoke this machinery for BF2's reopen and
selected the documented fallback instead: leave the cell open, do not
claim the exit passed, track the resolution as debt.

## Durable owner

Whichever future block first performs its own performance-lock exit that
depends on official-compiler-invocation-and-golden-generation at scale —
most likely **BV1** or **BS1** (the Vue/Svelte production-backend blocks;
`program-dag.toml`: `B4 -> {BV1, BS1}`, both downstream of BF2/BF3), since
their own acceptance evidence is the first point in the program where
comparing Verter's own candidate output against official-compiler goldens
becomes a real, non-bootstrap workload with a genuine independent baseline
candidate (Verter's compiler output vs. the official goldens BF2's harness
produces). If neither BV1 nor BS1 turns out to be the actual first
consumer at scale, the block that is must adopt this debt row instead —
this is not a fixed assignment to one specific block name, it is an
assignment to "whichever block's own performance-lock exit first requires
this workload locked," named here as the most likely candidates.

## Resolution gate

Before that owner's own performance-lock exit is accepted, it must freeze
`BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` (or a successor cell id
covering the same workload) through a genuinely independent measurement —
either the consult's bootstrap protocol above (if the maintainer ratifies
it at that time) or another maintainer-ratified procedure satisfying
governance.md's Gate authority rule. It may NOT reuse BF2's invalidated
10-run session or its derived numbers as inputs.

## Acceptance ID

`FC-PERF-001` — "the official-compiler-invocation-and-golden-generation
workload has a performance-gates.toml cell frozen through a process that
does not let the candidate choose its own pass criteria." Not satisfied by
BF2. Owned by the block identified above.

## Current state (as of this record)

- `performance-gates.toml`: no `[[cell]]` block exists for
  `BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE` — the row is
  explicitly marked OPEN/NOT YET LOCKED in a comment at the position the
  invalid cell previously occupied.
- `docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md`:
  updated to describe the attempt-then-invalidation and point back to this
  debt record.
- The invalidated 10-run raw session remains at
  `docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/`
  for audit only — not reusable as calibration input.
