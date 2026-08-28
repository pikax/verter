# C1 AC-5 / GAP-3 authority — unprimed architecture ruling

Dispatch metadata:

- input id: `C1-AC5-AUTHORITY-2026-08-26-01`
- model: `gpt-5.6-sol`
- reasoning effort: `xhigh`
- transport: Codex CLI `codex exec`, read-only sandbox
- candidate worktree: `/Users/carlosrodrigues/Documents/dev/verter-c1`
- candidate SHA: `b82ddb421480eef4718a8a0defaa254b7c946180`
- candidate tree: `d3c908ac58cd3bc9300d30bcebba5f0ba5d92705`
- registered integration authority: `b9a1b5b2f5e6d689de89447ebc00cc37f9f6453b`

You are the delegated, independent architecture authority for one C1 question. Read-only: modify nothing. This is a ruling, not implementation advice.

Before reading the question or candidate options, read and enumerate the binding invariants over this surface from:

- `CLAUDE.md`
- `docs/arch/refactor/rev11/charters/C1.md`
- `docs/arch/refactor/rev11/rulings/ARCH-ADDENDUM-C1-THREE-GAPS.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md`
- the registered performance-authority ruling inherited from integration
- `docs/arch/refactor/rev11/program.md`
- `docs/arch/refactor/rev11/program-dag.toml`
- relevant successor charters and program-state rows
- `docs/arch/refactor/rev11/orchestration/{roles.md,delivery.md,review.md}`
- `.claude/skills/type-resolution/SKILL.md`

Then inspect the exact candidate/evidence and relevant current source:

- `docs/arch/refactor/rev11/evidence/C1/ac-map.md`
- `docs/arch/refactor/rev11/evidence/C1/rebase-proof.md`
- `docs/arch/refactor/rev11/evidence/C1/scoping-spec.md`
- `docs/arch/refactor/rev11/evidence/C1/suite-results.md`
- `docs/arch/refactor/rev11/evidence/C1/disposition-consult.md`
- current `ResolverObservation`, `ResolverAttemptView`, `AttemptOutcome`, `ModuleResolverCore`, and TypeInfo/type-resolution entry surfaces

## Verified situation

- C1-AC-5 requires `AttemptOutcome::{Complete, NeedInputs, Terminal}` to cover every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt.
- The charter's original exhaustive-trait proof exists for the resolver observation surface: the sealed `ResolverObservation` methods return `AttemptOutcome<T>`, its test double is exhaustive, `ResolverAttemptView` is the single production implementation, and C1-AC-9's module-resolver I/O conversion is evidenced.
- The ratified three-gaps addendum replaces that proof for GAP-3 with one closed non-overridable inherent gateway: `TypeInfoCore::attempt(NonFlowOperation)`, with an exhaustive no-wildcard match and compile-fail privacy/alternate-entry rails.
- No `TypeInfoCore` or `NonFlowOperation` symbol exists in the candidate. The Stage-2 ruling's executable carrier omitted this non-flow TypeInfo convergence and recorded that Stage 2 was not the entire C1 charter.
- The current AC map therefore marks C1-AC-5 PARTIAL/OPEN. The registered performance waiver and C2 A6 continuation obligation cover only wall performance and future cross-snapshot continuation; they do not cover GAP-3.
- The clean rebase preserved all C1 production blobs and inherited registered authority exactly. No final candidate is frozen and no review has begun.

## Question

What is the production-ready architecture disposition for C1-AC-5/GAP-3 now, without fabricating coverage from the performance ruling or silently changing the charter?

Candidate options below are deliberately non-exhaustive, equally available, and unattributed. They are not the only options and none is preferred. Rule for another option if better.

1. Keep C1 blocked and implement the addendum literally now: introduce the closed `NonFlowOperation` enum and inherent `TypeInfoCore::attempt` gateway, route every C2-reachable non-flow TypeInfo operation through it, add exhaustive/privacy/alternate-entry tests, and rerun C1 acceptance before review.
2. Preserve the completed C1 module-resolver cutover and issue a narrow explicit exclusion/deferral for the not-yet-present TypeInfo gateway, binding the full GAP-3 obligation to a named existing/planned successor owner and retaining the resolver-side C1-AC-5/C1-AC-9 proof as C1's current acceptance surface.
3. Amend or rescope C1/C2 ownership or the landing boundary in another ratified way without either implementing GAP-3 now or silently declaring the absent gateway complete.
4. Any better disposition derived from the invariants, current consumers, and DAG.

## Required ruling

Answer all of these:

1. Which disposition is correct, and why?
2. Is `TypeInfoCore::attempt(NonFlowOperation)` required for any operation actually reachable from C2 on the current candidate, or is it a future contract with no current consumer? Cite the complete current-tree reachability evidence.
3. If implementation is required now, state the minimum exact variants/owners/callers/deletions/tests and why they are within C1 rather than an invented surface.
4. If exclusion/deferral is valid, state exactly what part of C1-AC-5 is accepted now, what remains unmet, the named existing/planned owner and acceptance ID, the deadline/gate, and whether C1 may proceed once the act is registered.
5. State whether any performance-authority or C2-continuation bytes may be reused; reject reuse if the obligation differs.
6. Provide exact repository authority artifact/registry/program-state/charter steps needed to make the ruling operative. Do not self-register or edit files.

Cite named invariants and concrete repository `file:line` evidence. Do not infer a preferred answer from option ordering. If the evidence cannot determine the complete reachable operation universe or successor owner, return FAIL and state the exact missing evidence/maintainer act rather than inventing it.

End with exactly:

===VERTER-RECEIPT-BEGIN===
LANE: c1-ac5-architecture-authority
RESULT: <PASS|FAIL>
REVIEWED: b82ddb421480eef4718a8a0defaa254b7c946180
FINDINGS: <n, or none>
FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
===VERTER-RECEIPT-END===

`PASS` means a complete lawful disposition can be registered without unresolved ambiguity. `FAIL` lists each blocking ambiguity or missing authority/evidence row.
