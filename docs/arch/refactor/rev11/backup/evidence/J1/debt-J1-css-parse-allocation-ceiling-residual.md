# Tracked debt — J1-CSS-ALLOC-001: the CSS parse-allocation ceiling residual

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition").

## What the finding is

`block/css-parser-allocation` — owner #1 of the CSS allocation split — reduced
`parse_style_ir` / `StyleSyntaxIr` allocation density substantially and then, in
its final commit (`de2021491`), **escalated rather than claimed success**:

> This block does not bring the pipeline inside J1's 1.2x per-category
> allocation-count ceiling, and no work inside `verter_css_syntax` will.

With the caller's `Arc::from(code)` admission copy added back, `parse_style_ir`
alone — zero planning, zero codegen, zero reparse — measured against legacy's
WHOLE-pipeline allocation count:

| category | parse-only (incl. admission) | legacy whole pipeline | ratio |
|---|---|---|---|
| descendant_selectors | 508 | 374 | 1.36x |
| pseudo_selectors | 458 | 374 | 1.22x |
| deep_rules | 808 | 525 | 1.54x |
| slotted_rules | 808 | 475 | 1.70x |
| global_rules | 808 | 373 | 2.17x |

Before that block those ratios were 1.42x–3.53x. The reduction is real. The
residual is the owned, cloneable, span-addressable `StyleSyntaxIr` tree itself —
one `Vec` per selector list, complex selector, compound, value list, statement
list, and selector-sink token buffer. Removing it means changing the IR's
ownership model to a bump arena, which owner #1's mandate does not authorize and
which would weaken the IR `ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP`
protects.

Owner #1 correctly touched nothing it was not authorized to touch: **no
`performance-gates.toml` cell was added, no bound was rebased, and no ratio was
adjusted to fit the result.**

This finding is **inherited**, not introduced by the slice-2 integration. The
integration composes owner #1's work onto `block/j1-integration` unchanged; it
neither improves nor worsens the residual.

## Ruling reference

Architecture consult (`codex`, `gpt-5.6-sol`, `model_reasoning_effort=xhigh`),
2026-08-24, dispatched for this disposition. Issued twice: the first ruling named
a new predecessor DAG block as durable owner, which
`MAINTAINER-RULING-2026-08-23-CSS-WORK-REACHES-J1` forbids ("no new DAG blocks");
the consult was re-briefed with that constraint supplied and re-issued. Second
ruling, verbatim:

> DEFER — durable owner: slice 4's cutover reconstruction within
> `block/j1-integration`.
>
> **Debt row — Owner:** slice 4 cutover reconstruction. **Resolution gate:**
> immediately after cumulative slice 3 evidence and before slice 4 acceptance or
> any legacy deletion. **Acceptance:** all eleven per-category allocation-count
> canaries pass at `<=1.2x`; alternatively, the locked bound is formally amended
> through the architecture ruling's independent-benchmark-error procedure, with
> retained calibration, independent review, and a complete evidence rerun.
>
> If slice 4 cannot satisfy that criterion, its owner must escalate at the
> pre-acceptance gate. The maintainer alone decides, through a fresh ruling,
> whether to:
>
> 1. authorize the `StyleSyntaxIr` bump-arena ownership change and keep slice 4
>    open until the canaries pass;
> 2. authorize a bound rebase only through the prescribed benchmark-error
>    procedure; or
> 3. leave the lock unchanged, in which case J1 does not land.
>
> Absent that ruling, the default is the third outcome. Neither completing
> slice 3 nor landing slice 2 creates authority to choose the first two.
>
> Folding the residual into slice 4 does weaken the structural separation behind
> the earlier warning about obscured ownership. The superior no-new-block ruling
> requires that compromise. Accountability is preserved by keeping this as a
> separately named debt row within slice 4, with its own evidence bundle,
> explicit assignee and independent reviewer, and a hard sub-gate before legacy
> deletion. Slice 4 owns resolution, not merely observation; J1 acceptance
> remains the final enforcement gate and cannot waive the miss.

The first ruling's substantive findings — that the disposition is DEFER, that
this is distinct from a forbidden "too expensive to fix" relaxation because the
ceiling remains binding and a slice landing is not acceptance, and that owner #1
itself has no defect to answer for — were re-affirmed unchanged and are recorded
here as part of the ruling.

## Durable owner

**Slice 4, the cutover reconstruction, within `block/j1-integration`.** Not a new
block: `MAINTAINER-RULING-2026-08-23-CSS-WORK-REACHES-J1` requires all remaining
CSS work to integrate into `block/j1-integration`, which lands as the single J1
block, with the 41 acceptance IDs unchanged.

This does fold owner-#1 residual ownership into the cutover, weakening the
structural separation `ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP`
warned about ("folding both into the already-large cutover would obscure
ownership"). The superior no-new-block ruling forces that compromise. It is
compensated, per the consult, by this being a **separately named debt row** with
its own evidence bundle, explicit assignee and independent reviewer, and a hard
sub-gate before legacy deletion — slice 4 owns *resolution*, not observation.

## Resolution gate

Immediately after cumulative slice 3 (`block/css-arena-lifecycle`) evidence
exists, and **before** slice 4 acceptance or any legacy deletion. No later than
J1 plan close. J1 acceptance is the final enforcement gate and cannot waive the
miss.

## Acceptance criterion

`J1-CSS-ALLOC-001` — all eleven per-category allocation-count canaries pass at
`<=1.2x`; **or** the locked bound is formally amended through
`ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP`'s independent
benchmark-definition-error procedure, with retained calibration, a lock-record
amendment, independent review, and a complete evidence rerun.

If neither holds at the pre-acceptance gate, slice 4's owner escalates and the
**maintainer alone** chooses between authorizing the bump-arena ownership change,
authorizing a bound rebase through the prescribed procedure, or leaving the lock
unchanged — in which case J1 does not land. **Absent a fresh maintainer ruling
the default is that J1 does not land.** Neither completing slice 3 nor landing
slice 2 creates authority to choose otherwise.

## Why this is not the forbidden relaxation

`ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP` Decision 3 states that
"'too expensive to fix' is not a valid reason to relax an approved
architecture/performance contract." This deferral is distinct: the ceiling
**remains binding and untouched**, no cell was added, no bound was rebased, and a
slice landing into the integration branch is not an acceptance event. The
deferral only sequences the ownership-model decision after owner #2's evidence
exists. It does not permit a cutover on a miss.

## Current state (as of this record)

- `performance-gates.toml` — unchanged by owner #1 and by this integration. No
  cell for the per-category CSS allocation ceiling was added or rebased.
- Owner #1's own evidence, carried in by the integration:
  `docs/arch/refactor/rev11/evidence/J1/intra-parser-allocation.md` (the
  escalation, the measured split, and a discrimination record naming three of its
  own assertions as non-discriminating controls and one plant that failed to
  apply and reported a false pass before being re-planted RED) and
  `docs/arch/refactor/rev11/evidence/J1/allocation-phase-attribution.md` (the
  donor baseline study, banner-bound to its origin and explicitly not reproducible
  against this tree).
- Slices 3 and 4 are not dispatched. The 41-acceptance-ID coverage map is not
  started.

## Re-measurement on the composed tree (supersedes the numbers above as current state)

Measured at `dd20f5c31e7c4fdf024e2b173a62c63d0f6fb4e1` via
`cargo nextest run -p verter_compiler --test allocator_canaries --no-capture`, 14/14 passing.
The table earlier in this document is owner #1's measurement of its own branch **in isolation**;
both of its columns have since moved, so it is retained as the escalation's original evidence and
is NOT the current state.

Parse-only allocation count (`ir_total` + the one admission call) against the legacy WHOLE-pipeline
count, all eleven categories:

| category | parse-only | legacy whole pipeline | ratio |
|---|---|---|---|
| v_bind_rules | 408 | 929 | 0.44x |
| v_bind_dotted | 408 | 929 | 0.44x |
| selector_lists | 658 | 822 | 0.80x |
| class_rules | 409 | 422 | 0.97x |
| repeated_classes | 359 | 371 | 0.97x |
| mixed_vue | 639 | 648 | 0.99x |
| pseudo_selectors | 408 | 371 | 1.10x |
| descendant_selectors | 458 | 371 | 1.23x |
| deep_rules | 758 | 522 | 1.45x |
| slotted_rules | 758 | 472 | 1.61x |
| global_rules | 758 | 370 | 2.05x |

Four categories exceed 1.2x, not five: `pseudo_selectors` has fallen below the ceiling (1.22x →
1.10x) and every other listed category improved. **The DEFER is unchanged** — four categories still
miss, and this comparison is a LOWER BOUND: it charges parse only, against legacy's entire pipeline.
The full-pipeline ratio the locked gate actually measures is worse than every row above, so nothing
here supports rebasing the bound, and none was rebased.

Two facts worth recording for whoever resolves this:

- **The legacy baseline itself moved.** All five of the categories cited in the original escalation now
  measure a legacy count 3 lower than recorded. Any future comparison must re-measure both columns
  on the tree under test rather than reusing either recorded column.
- **The per-rule scaling bound holds on the composed tree.** `per_rule_growth` is 1.008
  (`class_rules`), 1.002 (`deep_rules`), 1.005 (`selector_lists`) against the 1.10 bound, so the
  constant-cost claim survives composition.

The incremental derivation of `comment_spans` / `unpaired_cdo_span` added at
`dd20f5c31` contributes nothing to these numbers: `finish` measures 0 calls / 0 bytes in every
category, because the canary generators contain no comments.
