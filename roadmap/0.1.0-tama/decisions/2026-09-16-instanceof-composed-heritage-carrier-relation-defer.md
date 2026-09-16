# DEFER ruling and debt row: the `instanceof` structural fallback over a composed heritage carrier

- Status: proposed — awaiting maintainer ratification
- Date: 2026-09-16
- Adds: no DAG node. Records the debt row the D10 candidate leaves behind so the
  deferral is a disposition, not a TODO.
- Scope: one residue of D10's gap-vs-fallback split, following the convention of
  `2026-09-14-slot-binding-optionality-defer.md`.

## Context

D10's charter names a split: "A structural twin with no heritage relation takes
the checker's subtype fallback; an arm whose heritage cannot be decided (opaque
carrier, unresolved import) stays gapped and never warm" (D10-AC2, "fallback and
gap respectively").

The candidate closes that split. The `instanceof` heritage walk now reports
CHAIN COMPLETENESS (`HeritageReading::decided`) beside the ancestor chain, so an
empty chain is separated into its four causes — a proved heritage-free class, an
unreadable declaration, a non-class declaration, and a class whose `extends`
clause is an expression the fact producer could not name. An arm PROVED
unrelated in both directions now falls to the checker's own union-level
fallback, in the checker's order (the instance type when it is a subtype of the
remaining subject, the remaining subject when that subject is assignable to the
instance type, the whole-subject intersection otherwise), and publishes clean
and warm; an arm whose ancestry could not be decided keeps the typed
`FlowGap(GuardNarrowing)` and never warms. Both halves are pinned by
`instanceof_heritage_decides_both_edges_and_bounds_its_reads` (case 4) and
`instanceof_undecidable_heritage_stays_gapped_and_never_warms`.

One arm direction stays gapped, for a reason that is not the heritage walk's.

## Debt row — `INSTANCEOF-COMPOSED-HERITAGE-CARRIER-RELATION`

- **Finding:** when the TESTED class itself has heritage, its carrier unwraps to
  the heritage INTERSECTION (`class KSub extends K { s = 1 }` unwraps to
  `K & { s: number }`, not to a composed `{ k: number; s: number }` surface).
  The shared relation authority answers a class-vs-class pair through that
  carrier undecided in one direction: a source-side intersection is decided by
  alternatives over its members (no member alone carries the whole surface, so
  the pair reads `NotAssignable`), and a target-side intersection distributes to
  members that are still `DeclRef` carriers, which the recursive pair level
  classifies as deferred and answers `Unknown`. An undecided relation is no
  fact, so the evaluator does not enter the checker's fallback on it: for a
  shape-identical, heritage-free `Twin` under `x instanceof KSub`, the checker
  publishes `KSub` through `isTypeSubtypeOf(candidate, type)` and the candidate
  publishes the unchanged subject behind the typed guard gap, never warm. The
  answer is a superset, `ReturnOnly` — never a wrong-complete result — and the
  same guard over a heritage-FREE tested class does publish the checker's
  fallback clean and warm.
- **Why deferred:** the missing capability is the RELATION authority's, not this
  evaluator's. Deciding a class-vs-class structural pair through a composed
  heritage carrier means changing how `expand_pair` reduces intersection
  operands and how the recursive level classifies an unexpanded identity
  carrier — the relation authority and the nominal relation facts landed by D3R
  are a NAMED BOUNDARY of D10's charter, and its forbidden designs exclude
  deciding narrowing outside them or standing up a second relation. D10 must not
  re-implement that reduction, and widening the charter to carry it would put
  public relation semantics in the same change as the guard-narrowing arms.
- **Durable owner block:** `rev11.flow:sole shared flow authority` — the owner
  declared by `D3R — Nominal relation authority`
  (`roadmap/0.1.0-tama/charters/rev11-flow/D3R.md`), the DAG node that owns
  `crates/verter_session/src/project_semantic_dispatch/relation.rs`, its
  intersection-operand reduction, and the nominal relation facts D10 names as a
  boundary. D10 declares the SAME owner, which is why this row can be assigned
  now rather than left unowned: it stays inside the authority that already owns
  the surface, and only its resolution gate waits on ratification. The row is
  NOT assigned to any diagnostics consumer of the relation (the
  `expansion.native-checker` families consume `Relate` outcomes and explicitly
  leave relation facts with their existing semantic authority).
- **Resolution gate:** the next `rev11.flow` node that revises the relation
  authority's intersection-operand reduction, and no later than plan close — so
  the relation semantics change once.
- **Acceptance ID/test:** the retirement flips case (5) `carrierTwin` of
  `instanceof_heritage_decides_both_edges_and_bounds_its_reads`
  (`crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs`)
  from `FlowGap(GuardNarrowing)` with zero warm candidates to the checker's
  `0 | KSub`, clean and warm — the same assertion case (4) already makes for the
  heritage-free tested class. Until then that case pins the current contract:
  the undecided structural relation degrades, and it never guesses.
- **Ruling reference:** this decision (D10 candidate review, adversarial /
  conformance P2 on the gap-vs-fallback split).

## Decision

1. The debt row above is the disposition of record for the composed-heritage
   carrier's undecided structural relation; it is not a TODO in source.
2. D10's own acceptance is met by the heritage-free twin control (fallback,
   clean, warm) and the undecidable-heritage controls (gap, never warm); this
   row records the arm the relation authority still cannot decide.
3. The row is OWNED as of this decision — `rev11.flow:sole shared flow
   authority`, D3R's owner — satisfying D10's "P2 needs a named owner when
   deferred". Ratification fixes its resolution gate against a specific
   successor; until then it is an open deferral counted at plan close, but not
   an unowned one.
