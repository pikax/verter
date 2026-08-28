# C1 performance authority — unprimed architecture ruling

Dispatch metadata:

- input id: `C1-PERFORMANCE-AUTHORITY-2026-08-26-01`
- model: `gpt-5.6-sol`
- reasoning effort: `xhigh`
- transport: Codex CLI `codex exec`, read-only sandbox
- candidate worktree: `/Users/carlosrodrigues/Documents/dev/verter-c1`
- candidate SHA: `7ddbba827e15b9698850a7e01c21a9e41638aec3`
- candidate tree: `bb6dcd1908b3b81c5350ed777e1051b12cdc3a62`
- integration base: `d1f3d50a948597f036868543b9bb21acacd730ff`

You are the delegated, independent architecture authority for one C1 question. Read-only: modify nothing. This is a ruling, not implementation advice.

Before reading the question or candidate options, read and enumerate the binding invariants over this surface from:

- `CLAUDE.md`
- `docs/arch/refactor/rev11/charters/C1.md`
- `docs/arch/refactor/rev11/rulings/ARCH-ADDENDUM-C1-THREE-GAPS.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md`
- `docs/arch/refactor/rev11/contracts/input-loading.md`
- `docs/arch/refactor/rev11/orchestration/{roles.md,delivery.md,review.md}`
- `.claude/skills/type-resolution/SKILL.md`
- `.claude/skills/rust-performance/SKILL.md`

Then inspect the exact candidate and evidence:

- `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/frontier-resume-architecture-consult.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/unblock-architecture-consult.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/residual-244-diagnostic.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/receipt.md`
- relevant code at candidate SHA, especially `verter_workspace::resolver`, `ResolveFrame`, `AttemptOutput`, priority-frontier handling, and the locked A6 harness/configuration

## Verified situation

- The exact enabled A6 arm passes: all semantic/output/counter/digest locks hold; normalization is 1,981 against a maximum of 11,313.
- A valid isolated, cooldown-controlled A/B/B/A wall protocol measured base 86.685 ms and candidate 96.450 ms: +11.264925%. Candidate passes the 100 ms absolute cap; RSS passes. The relative 3% cap fails. Candidate retired instructions are +10.004375% and cycles +7.460397%.
- Bounded C1-local pure-data optimization recovered 416,326,964 instructions and remains committed. The candidate still needs to remove 3,273,612,760 instructions to meet the 3% cap.
- Exact call accounting shows base 724 resolves/724 probes versus candidate 2,172 fresh attempts and 19,548 ordered probe-frontier operations. The remaining excess is the three-wave whole-operation restart required by the current input-loading, AttemptOutput-discard, frontier, and witness/replay contracts.
- Independent analysis found no further lawful pure geometry/Arc/scratch-buffer optimization capable of closing the gap. Cross-request caching was rejected. Cross-snapshot frontier-prefix continuation was rejected under the current contract because it retains/replays observation-dependent semantic output across attempts.
- The maintainer has made a narrowly scoped C1 performance waiver an admissible action if present-authority optimization is exhausted. This fact establishes decision authority; it is not a requested conclusion. No threshold may be silently reweighted and no evidence may be restamped.

## Question

What is the production-ready architecture disposition for C1's relative wall-time blocker, given the proved conflict between the current restart/discard contract and the 3% relative gate?

Candidate options below are deliberately non-exhaustive, equally available, and unattributed. They are not the only options and none is preferred. Rule for another option if better.

1. Keep the relative 3% gate blocking C1 and amend C1 now with a typed, snapshot-revalidated semantic continuation contract, including the required input-loading, `AttemptOutput`, F18 frontier, F24 witness/replay, invalidation, and private state-machine API changes; then implement and remeasure before review.
2. Preserve the current restart/discard semantics for C1; issue a narrow, explicit one-candidate waiver of only the relative wall-time blocker, retain the absolute 100 ms, RSS, enabled counters/digest, correctness, review, mutation, and canonical landing gates, and bind the measured regression plus continuation/performance work to a named existing/planned successor owner.
3. Keep C1 blocked or alter its landing boundary/subject in some other ratified way without authorizing semantic continuation or waiving the relative gate.
4. Any better disposition you derive from the invariants and evidence.

## Required ruling

Answer all of these:

1. Which disposition is architecturally correct, and why?
2. Does it expand C1's charter? If so, name the existing/planned owner; do not invent an owner. If none exists, state the exact maintainer act required.
3. If a waiver is valid, state exactly what is waived, for which candidate/tree/evidence identity, what remains binding, what successor obligation/owner/acceptance evidence is required, and whether C1 may proceed to final-candidate completion and review once the act is registered.
4. If continuation is required now, state the minimum ratified contract and acceptance additions before implementation; do not provide a vague optimization suggestion.
5. State whether committed request-local cleanup `0c22953821f57eedd32b812b1478a449a976f964` should remain in C1.
6. Provide the exact repository authority artifact/registration steps needed to make the ruling operative. Do not self-register or edit files.

Cite a named invariant and concrete repository `file:line` evidence for every conclusion. Do not infer a preferred answer from option ordering or the maintainer's admissibility statement. If evidence is insufficient, say what exact evidence is missing.

End with exactly:

===VERTER-RECEIPT-BEGIN===
LANE: c1-performance-architecture-authority
RESULT: <PASS|FAIL>
REVIEWED: 7ddbba827e15b9698850a7e01c21a9e41638aec3
FINDINGS: <n, or none>
FINDING <id> | <P0|P1|P2|P3> | <file>:<line> | <one-line summary>
===VERTER-RECEIPT-END===

`PASS` means you provide a complete lawful disposition that can be registered without unresolved architecture ambiguity. `FAIL` lists each blocking ambiguity or missing authority/evidence row.
