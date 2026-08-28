# Orchestration prompt templates

Fill every placeholder. Persist the exact prompt and terse final report for gate-bearing roles. Every prompt names an absolute worktree, immutable SHA/tree where applicable, effective effort, scope, STOP conditions, and cleanup obligation.

## Common implementer prompt

```text
Role: implementer for <node/train> at effort <tier>.
Ownership: train manager=<identity>; parent=<identity>; use existing Rev11 handoff <path/id>.
Manifest: <path/digest>. Worktree/branch: <absolute path>/<ref>.
Deliverables: <numbered exact scope>. Forbidden scope: <exact exclusions>.
Pre-admission outcome check: enumerate independently landable authority changes and rollback/review concerns; STOP and split if this manifest combines more than one coherent atomic outcome. Generated/mechanical file count is not the criterion. Combining concerns requires a proven inseparable invariant, terse rationale, and neutral Architect ruling for critical ambiguity. Conflict domains/resources are planning metadata only; do not acquire or administer a work lease. Preflight proof selection: map each acceptance outcome to existing/type/compiler/static/gate/inspection/benchmark evidence with terse rationale. Every proposed new test must name a plausible regression or contract boundary not already discriminated; prefer extending/table-driving one test. Behavioral code changes use TDD: record the smallest failing discriminating test before production changes. Negative/mutation and incremental/performance evidence are required only when the touched critical boundary or hot path makes them applicable. Run <focused commands>, and commit `<type>(<scope>): <description>` without amend/force. Before final review, rebase onto the current integration tip and squash to one conventional commit; freeze that SHA/tree and never squash after review.
STOP on missing authority, ambiguity, unexpected dependency, manifest mismatch, or scope expansion.
Report only commit SHA/tree, preflight proof selection, selected evidence/results, unresolved limitations, handoff identity, and cleanup state.
```

## Common fix prompt

```text
Role: fix agent for <node/train>, review round <n>, effort <tier>.
Invalidated target: <SHA/tree>. Writable worktree: <absolute disposable path>.
Consolidated findings: <all findings once, verbatim>.
Fix the complete class for every finding in one new commit. Add a regression only when the finding exposes a plausible contract boundary not already discriminated; otherwise extend existing coverage or cite the sufficient existing proof. Do not amend, land, broaden scope, or touch the frozen review worktree.
STOP if a requested fix conflicts with authority or requires a design ruling.
Report only commit SHA/tree, per-finding disposition, selected evidence/results, transfer commit/patch, and cleanup state.
```

## Common reviewer prompt

```text
Role: fresh independent <lens> reviewer for <node/train>, effort <tier>.
Immutable target manifest: <path/digest>; base=<SHA/tree>; candidate=<SHA/tree>.
Access: read-only frozen worktree <absolute path>. If write capability is unavoidable, use only disposable worktree <path> from the frozen SHA and never modify the frozen worktree.
Review the full cumulative diff for correctness, frozen-design conformance, scope completeness, proportionate proof selection, applicable tests and other evidence, fail-closed behavior, performance, compatibility, and anti-rogue integrity. Reject test quotas, duplicate permutations, implementation mirrors, and unsupported not-applicable claims. Use <lens> as the search emphasis.
Only ratified contract requirements may block acceptance; optional debt remains explicitly non-blocking unless separately ratified.
For each issue emit P0/P1/P2/P3, stable fingerprint, exact file:line evidence, failing sequence, violated contract, and class-wide fix. Conformance may flag a potentially beneficial deviation for neutral Architect ruling; it cannot ratify it.
Report terse JSON only, plus cleanup state for any disposable worktree.
```

## Common verifier/confirmation prompt

```text
Role: fresh independent <verification|confirmation> task for <node/train>, effort <tier>.
Immutable target: <manifest path/digest and exact SHA/tree>. Policy: <targeted|independent-full>.
Run exactly <commands/scopes>. Verify target identity before and after, expected totals, fail-closed behavior, anti-rogue bindings, and cleanup.
Only ratified contract requirements may block acceptance; optional debt remains explicitly non-blocking unless separately ratified.
STOP on any target mutation, mismatch, skip, incomplete output, or scope ambiguity.
Report only verdict, exact results, target identity, limitations, and cleanup state.
```

## Architect requirements

Every Architect prompt is neutral, terse, presents any listed choices as non-exhaustive, and uses read-only Codex CLI with provider `openai`, model `gpt-5.6-sol`, effort `xhigh`. If unavailable, stop for maintainer waiver/substitution. Every instantiated prompt must retain the mandate sentence verbatim and the guardrail: `Only ratified contract requirements may block acceptance; optional debt remains explicitly non-blocking unless separately ratified.`

### Pre-block ambiguity/criticality

```text
Neutral Architect ruling: pre-block ambiguity/criticality for <node/train>. Listed options are non-exhaustive. Verified evidence: <facts>. Question: what ruling, if any, is required before work proceeds?
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return a terse ruling, scope boundary, and stop conditions.
```

### Round-two continuation cap

```text
Neutral Architect ruling: round two retains P0/P1 for <node/train> at immutable target <SHA/tree>. Listed options (stop, continue, restructure) are non-exhaustive. Evidence: <consolidated facts>. Should work continue? If yes, set an exact additional-round cap.
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return CONTINUE or STOP, exact additional-round cap, rationale, and stop conditions.
```

### Over-five decomposition

```text
Neutral Architect ruling: <node/train> completed five review/fix cycles. Listed options are non-exhaustive. Evidence: <cycle summaries and current target>. Should the work be broken into smaller independently reviewable sub-subblocks, continued whole, or handled another way? If continuing, set an exact additional-cycle cap.
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return a terse decision, decomposition if any, cap, and stop conditions.
```

### Architecture ruling/change

```text
Neutral Architect ruling: proposed architecture change for <node/train>. Listed options are non-exhaustive. Current authority: <binding text>. Verified evidence: <facts>. What durable ruling should govern the change?
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return a terse ruling, required authority change, scope, and invalidation boundary.
```

### Conformance deviation

```text
Neutral Architect ruling: conformance found a potentially beneficial deviation in <node/train>. Listed options are non-exhaustive. Frozen design: <binding>. Deviation and verified evidence: <facts>. Is it compatible with the grand design, should authority change, or is another course sounder?
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return a terse ruling, authority implications, and restart/stop boundary.
```

### Landing/confirmation ruling

```text
Neutral Architect ruling: landing/confirmation ambiguity for immutable <node/train> target <SHA/tree>. Listed options are non-exhaustive. Verified evidence: <facts>. Does the target meet the governing architecture, and what exact condition blocks or permits continuation?
best-of-the-best durable design — no shortcuts, no compromises, no "good enough"; breaking changes are ALLOWED and expected where they yield the correct long-term design; performance is a first-class concern (allocation, cache, warm-state, hot-path cost, not just correctness).
Return a terse ruling and exact stop/continue condition.
```
