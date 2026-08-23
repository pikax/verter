---
ruling_id: "CSS-ALLOCATION-OWNERSHIP-2026-08-23"
type: "architecture-ruling"
date: "2026-08-23"
date_source: "in-document (**Date:** 2026-08-23)"
binds: ["J1"]
source_file: "ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP.md"
summary: "Splits the CSS allocation regression into two ADOPT-NOW predecessor blocks of the CSS cutover: a verter_css_syntax block owning parse_style_ir/StyleSyntaxIr allocation density (must optimise the shared parser, never route around it or add a second parser), and a verter_compiler block owning the per-:slotted() Allocator::new() lifecycle defect (requires an enabled requested-byte/scaling regression test, since the count-only canary cannot catch it). Rules block/css-closing-items unlandable as a preparatory dual-path migration under the J1 charter's prohibition on style_planner and legacy css/ coexisting as Vue transform authorities; it becomes a staging/evidence donor. Rules the 2.64x count / 5.54x requested-byte regression unacceptable to ship under the present contract, and refuses a ratio rebase absent proof of a material benchmark-definition error established independently of the candidate's miss."
supersedes: []
superseded_by: []
contradicts: []
notes: "Supersedes the debt row assigning the entire CSS allocation regression to block/css-cutover, and supersedes the earlier allocation DEFER only to the extent it was read as authorization to land block/css-closing-items on the protected branch; the DEFER remains valid as an unlanded/WIP disposition. Records three qualifications limiting the evidence: 5.54x is cumulative allocator-REQUESTED bytes (not peak/retained/RSS), Allocator::new() was not experimentally isolated from the rest of the planner phase, and the study's claim that fixing owner #1 would not move slotted_rules' bytes ratio is false. Explicitly states the ignored allocation canary is NOT a Stub Prevention violation (real body, honestly RED) but is equally not a passing acceptance gate."
---

# Architect ruling — CSS allocation ownership and the J1 landing sequence

**Date:** 2026-08-23
**Authority:** architecture consult (codex architect), acting under the delegated
amendment-ratification authority recorded for this program.
**Supersedes:** the debt row assigning the entire CSS allocation regression to
`block/css-cutover`, and any reading of the earlier allocation DEFER as
authorization to land `block/css-closing-items` on the protected branch.

## Evidence

`docs/arch/refactor/rev11/evidence/J1/allocation-phase-attribution.md` —
11-category phase attribution of the converged CSS pipeline against legacy.
Aggregate 2.64x allocation count, 5.54x requested bytes.

Three qualifications on that study, recorded so later readers do not overstate it:

- **5.54x is cumulative allocator-*requested* bytes**, including requested
  `realloc` sizes. It is not peak, retained, or RSS memory.
- **`Allocator::new()` was not experimentally isolated** from the rest of the
  planner phase. Occurrence scaling plus code inspection make the diagnosis
  compelling, but it remains phase attribution plus causal inference.
- The study's claim that fixing owner #1 would not move `slotted_rules`' bytes
  ratio is **false**. It would reduce the ratio substantially; it simply would
  not remove owner #2's independent per-occurrence floor.

These qualifications do not collapse the two owners into one.

## Decision 1 — two separate predecessor blocks (ADOPT-NOW)

Both are immediate predecessors of the final CSS cutover.

**1. `verter_css_syntax` allocation block.** Owns `parse_style_ir` /
`StyleSyntaxIr` construction and its internal allocation density. Its first task
is deeper attribution *inside* the parser: the current `parse` bucket also
includes the caller's `Arc::from(code)` admission copy
(`crates/verter_compiler/src/style_planner.rs:293`), though one copy cannot
explain hundreds of calls.

This block **optimises the shared parser**. It must not route around it, weaken
its IR, or introduce a second parser.

**2. `verter_compiler` arena-lifecycle block.** Owns the fresh
`oxc_allocator::Allocator` per `:slotted()` occurrence in
`render_special_argument` (`crates/verter_compiler/src/style_planner.rs:1706`)
and evaluates the related per-stage arena in `emit`. Eliminating the
occurrence-local floor is mandatory; pooling or reuse is a mechanism choice, not
the architectural contract.

It requires an **enabled requested-byte / scaling regression test** — the
existing count-only canary would not reliably catch this defect.

This placement follows the standing rule that a performance fix lands in the
lowest reusable owner crate. Folding both into the already-large cutover would
obscure ownership and make the shared-parser repair look compiler-local.

The cutover retains only the final integration obligation: all eleven
per-category canaries enabled and passing, followed by legacy deletion.

## Decision 2 — `block/css-closing-items` may not land as proposed

**No.** As a landed unit it is a preparatory dual-path migration.

The proposed split argues no dual path is created because no consumer switches
yet. That reading is too narrow: it lands replacement capability while the live
legacy NAPI-facing authority remains. The ratified J1 charter explicitly forbids
`style_planner` and the legacy `css/` implementation coexisting as Vue transform
authorities (`charters/J1.md:246`), and Legacy Code Deletion requires the
superseded implementation to disappear in the replacement change.

**The `#[ignore]`d allocation canary is NOT a Stub Prevention violation** — its
body is real and it is honestly RED. But it is equally not a passing acceptance
gate. The earlier DEFER remains valid as an unlanded/WIP disposition; it is
superseded only to the extent it was read as permission to land.

The proposed split is also internally stale: it states the allocation canary does
not gate Slice 2, while the ratified charter and the allocation ruling both block
legacy deletion on that gate.

Consequently the worktree remains a **staging/evidence donor**. Its work is
rehomed into the two allocation predecessors, the Svelte convergence block, or
the final atomic cutover.

## Decision 3 — the regression is not acceptable to ship now

Not acceptable under the present contract. A defensible product position may
exist later — that the canonical parser performs necessary reusable work, that
absolute latency and memory are acceptable, and that the historical ratio is the
wrong long-term guard — but the current evidence does not establish it:

- Owner #2 is a concrete lifecycle defect, not an architectural tax.
- Owner #1 has not been decomposed internally or shown irreducible.
- The locked gate is per-category allocation-**call count ≤1.2x** — not the 2.64x
  aggregate and not the 5.54x requested-byte diagnostic.
- There is no release-profile, real-corpus, peak-memory, or scaling evidence that
  the absolute product cost is acceptable.

**"Too expensive to fix" is not a valid reason to relax an approved
architecture/performance contract.**

Rebasing the bound is permissible only on proof of a material
benchmark-definition or equivalent-work error, established independently of the
candidate's miss, with retained calibration, a lock-record amendment, independent
review, and a complete evidence rerun. "The candidate builds a richer IR" is not
such proof — the charter deliberately selected that pipeline *before* setting the
bound. If parser cost later proves necessary and absolute budgets are green,
escalate for maintainer-approved rescope and replace the ratio with enabled,
pre-locked absolute and scaling guards. Do not merely raise the ceiling to fit
the observed result.

## Landing sequence

1. Record the DAG/charter amendment creating the Svelte convergence block and the
   two allocation predecessors. Without it, all three worktrees remain private
   layers and must combine into one final J1 candidate.
2. Land the **Svelte grammar convergence** block first: commit its outstanding
   work, delete the superseded Svelte grammar in the same block, prove byte-exact
   parity, restack on current protected trunk.
3. Land the **`verter_css_syntax` allocation** predecessor on the final
   shared-parser shape, including any parser-correctness changes that belong there.
4. Land the **`verter_compiler` arena-lifecycle** predecessor; rerun both count
   and requested-byte attribution.
5. Do **not** land `block/css-closing-items`. Use it as a donor.
6. Reconstruct the **cutover** on the cumulative trunk, absorb the remaining
   closing items, enable and pass all eleven canaries — or complete a valid formal
   recalibration — then switch every public route while deleting `lightningcss`
   and the legacy `css/` tree in one squashed commit.
