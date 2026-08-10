# A2C — Abrupt-completion facts for G10 safety discrimination

**Status:** PREPARED; begins only after A2 acceptance and ratification of AMD-002.  
**Class:** Foundational safety.  
**Predecessors:** A2.  
**Gate 0 lineage SHA:** `UNSET`; record the exact accepted A2-based candidate for this evidence block.

## Objective

Provide one content-free, exact-or-typed-unknown completion model sufficient for A3 to distinguish G10 from checker-correct clean results, without changing public semantic results or implementing the later sole flow solver.

## In scope

- canonical completion kinds `Normal`, `Return`, `Throw`, `Break(target)`, and `Continue(target)`;
- compositional completion-set transformation for blocks, `if`, labels, `switch`, loops, `try`, `catch`, and `finally`, limited to syntax-complete facts and typed unknown where final loop/flow semantics are required;
- structural authored-return membership and exact endpoint-`undefined` disposition;
- compact arena-free completion facts stored on `FunctionBodySkeleton`, computed once during skeleton construction and reusable without a query-time AST rewalk;
- an exact statement/suffix fact permitting A3 to identify the G10 abrupt-completion hazard without another syntax allowlist;
- discriminating fact-level tests for G10, labeled/switch/catch siblings, and the named checker-correct controls X68, X80, and X88.

## Out of scope

- public result retraction or any other semantic behavior change; A3 owns that behavior;
- closure reads/writes, capture summaries, escape/freshness analysis, or position-independent effect transfer; D5 owns those mechanisms;
- closure-escape, loop-summary, or `try`/`finally`-override graph edges, loop fixed points, slot transfer, or flow-state joins; D6 and `U6.LOOP_CLOSURE` own those mechanisms;
- proof-carrying complete-result construction, final obligation discharge, or warm-admission closure; D8 owns those mechanisms;
- a second syntax-shaped evaluator, a second control graph, cache admission, compatibility repair, or speculative services.

## Required evidence

Exact completion-set tests and pinned-checker discrimination for G10 plus labeled/switch/catch siblings; X68/X80/X88 remain exact clean controls; missing or unsupported facts produce typed unknown and never a guessed exact fact; construction is deterministic and linear, facts are `NoTypeExpr`, retained size is measured, and no query-time AST rewalk occurs; mutation recipes independently break label routing, throw-to-catch routing, and `finally` override and make the named tests fail.

## Abort/rescope

Stop if G10 discrimination requires value typing, capture/effect transfer, loop fixed-point state, graph-edge ownership, a second flow representation, or a public semantic change. Stop if a completion fact cannot be exact and fail-closed without guessing. Amend the charter rather than absorbing D5, D6, D8, or `U6.LOOP_CLOSURE`.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A2C` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA and proves both exact G10 discrimination and non-interference with public results.
