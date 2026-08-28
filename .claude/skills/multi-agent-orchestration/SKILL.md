---
name: multi-agent-orchestration
description: >-
  Drive a substantial implementation or multi-train migration through bounded implementation,
  immutable risk-scaled review, consolidated fixes, verification, landing, confirmation, and
  cleanup. Use for autonomous staged work, train management, or a large block needing independent
  fresh-agent review.
---

# Multi-Agent Orchestration

Use one parent orchestrator to own ordering, authority, landing, and cross-train state. A train manager owns only the train named in its frozen manifest: implementation dispatch, targeted verification, consolidated fixes, and a terse handoff. In nested execution, the parent owns review dispatch, acceptance, landing, and confirmation. Use the existing Rev11 handoff mechanism for that boundary; do not create a replacement ledger, transport, or control plane.

Rev11 ORC0 provides honest-operator, trusted-local consistency and audit evidence. It does not prove harness authenticity, malicious-owner resistance, or independent anti-rollback.

## Admission and effort

Admission freezes the authorized block/train manifest: exact node and scope, base and candidate-start SHA/tree, authority/charter/profile bindings, conflict domains, stop conditions, and ownership. Conflict domains and resource classes are planning metadata, not leases or scheduling authority; the maintainer coordinates concurrent ownership. A legacy `lease_id` may appear as an opaque lifecycle round handle but grants no exclusion or capacity right. Implementation, fix, review, verification, and confirmation are fresh provider-neutral harness tasks. The node's static per-role low/medium/high minima/defaults plus deterministic authority signals compute each role's effective tier automatically. An optional override may only raise it. Persist the exact task, provider, model, effort, prompt, and report bindings; do not add a manual assessment ceremony.

Before implementation, preflight maps every acceptance outcome to proportionate evidence. Tests are evidence, not quota: prefer applicable existing coverage, then extend or table-drive one test, and add a test only for a plausible regression or contract boundary not already discriminated. Type/compiler/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid with a terse rationale. Negative/mutation evidence is reserved for plausible critical fail-closed/correctness boundaries or reproduced defects. Incremental/performance evidence applies only when the train touches that authority or hot path. Behavioral code changes still use TDD.

Pre-admission also enumerates independently landable outcomes. Reject or split a block that combines multiple authority changes or independently rollbackable/reviewable concerns, such as root cutover, orchestration policy, lifecycle tooling, and product implementation. Generated or mechanical file count is not the criterion; one coherent atomic outcome is. Combine concerns only when separation would violate a proven atomic invariant, record the terse rationale, and obtain a neutral Architect ruling for critical ambiguity. The parent may land ordered subblocks consecutively, but must not turn them into one frozen review unit.

The sole Architect role is a neutral, read-only Codex CLI consultation using OpenAI `gpt-5.6-sol` at `xhigh`. Use it only for a real escalated ambiguity or ruling. If its budget is unavailable, only the maintainer may waive it or name a substitute.

## Ownership and stop boundaries

- The parent chooses train order, owns the integration branch and round-bound lifecycle evidence, schedules gate-bearing reviews/verification/confirmation, accepts transfers, lands, and reports external status.
- The train manager never selects another train, expands authority, lands, or continues past a STOP. It returns an exact commit/patch and evidence through the existing Rev11 handoff.
- Implementers and fix agents mutate only their assigned worktree and commit conventional commits. Reviewers are read-only unless assigned a disposable write-enabled worktree created from the frozen SHA.
- STOP on missing authority, scope ambiguity, invalid manifest, unexpected dependency, inability to produce sufficient discriminating evidence, immutable-target change, gate failure, or a requested action outside the brief. Report the concrete blocker tersely; do not invent a workaround or a new train.
- Keep external updates honest and terse. Persist gate-bearing artifacts, not conversational status or a commit per update.

When a train unlocks frontier, do not automatically select or launch another train. Ask the maintainer for ordering unless the launch prompt explicitly authorized continuous autonomous order. Even under that authorization, stay within its named ordering and stop boundaries.

## Worktree and immutable-review rules

Create one worktree/branch for the train. Rebase onto the current integration tip and squash the candidate to one conventional commit before finalization. Then record one frozen review-target manifest containing the exact base/candidate SHA/tree and worktree. The frozen train/block worktree must remain unchanged for the entire review round. Landing fast-forwards the exact reviewed commit; never squash after review.

A read-only reviewer may inspect the frozen worktree. Any reviewer or diagnostic task that can write receives its own disposable worktree created from the frozen SHA. A proposed change transfers back only as an explicitly accepted commit or patch after the current round is invalidated and a successor round is started. Never let review-side writes silently change the reviewed target.

Remove each disposable agent worktree immediately after its result is safely recorded and no transfer remains. Remove the train worktree after landing or abort. Every final handoff reports cleanup as `removed`, or names the still-required worktree and owner.

## Risk-scaled review

The review profile is frozen before dispatch and every reviewer targets the same immutable cumulative SHA/tree:

- Low/simple: one fresh adversarial reviewer.
- Medium: one or two fresh reviewers as selected by the profile; adversarial is mandatory and conformance is the preferred second lens.
- High/critical: exactly three distinct fresh tasks: adversarial, conformance, and one context-specific or ad-hoc specialist lens.

Adversarial is the default search posture. Conformance checks the implementation against the frozen design and authority. If conformance finds a potentially beneficial deviation, it may request a neutral `conformance-deviation` Architect ruling; it does not silently reject or ratify the deviation. Choose the specialist for the actual risk surface—public wire, compatibility, concurrency/lifetime, performance, migration, or another explicitly named expertise.

All reviewers inspect the full cumulative diff, preflight proof selection, applicable tests and other evidence, scope completeness, fail-closed behavior, performance implications, and anti-rogue integrity. They reject test quotas, duplicate permutations, implementation mirrors, and unsupported not-applicable claims. The author never reviews its own work. A profile-required clean current-round set is mandatory; high-risk work therefore requires clean 3/3.

## Review/fix loop

1. Freeze the exact candidate and manifest.
2. Dispatch all profile-required reviewers independently against that target.
3. Wait for all reports, then consolidate the complete round once.
4. If fixes are needed, invalidate the round. Dispatch one fix agent to address the whole consolidated set in one comprehensive new commit, including class-wide siblings. Add a regression only when the finding exposes a plausible boundary not already discriminated; otherwise extend existing coverage or cite the sufficient existing proof.
5. Freeze the successor target and rerun the complete risk-scaled review set. A changed SHA/tree never inherits a verdict.

Two completed review/fix cycles are the soft maximum; reconciled history and global round labels do not count. If the second completed cycle retains any P0/P1, stop automatic cycling and invoke the neutral Architect with the round-two-cap template. Continuing requires its explicit additional-cycle cap. Before a sixth completed review/fix cycle, the Architect must explicitly decide whether to split the work into smaller independently reviewable sub-subblocks, regardless of severity. P2/P3 follow the owning profile; never silently drop a real finding.

## Verification, landing, and confirmation

Run the preflight-selected targeted evidence during implementation/fixes and the owning canonical gate only on the final frozen target. Any content, rebase, regeneration, conflict resolution, or tree change invalidates review and gate evidence.

Confirmation is risk-scaled: high-risk work gets fresh independent full confirmation; medium gets targeted confirmation defined by its profile; low may omit confirmation when its profile says `not-required`. Integration confirmation is allowed only at an explicitly named convergence or dependency-join milestone, never because a periodic train count was reached.

Before landing, verify immutable target identity, required clean review set, verification, confirmation policy, anti-rogue constraints, and integration-tip stability. Transfer only accepted commits/patches, land byte-identically, record the durable receipt, and clean worktrees.

## Anti-rogue checks

Fail closed on stale/missing manifest bindings, inherited or duplicate reviewer identity, mismatched effort/model/provider/task/prompt/report, reviewer writes to the frozen worktree, result reuse across rounds, lower-than-required effort, hidden scope expansion, fabricated gates, or unknown cleanup state. Under the trusted-local model these are operator-audited consistency checks, not cryptographic proof against a malicious owner.

See `references/templates.md` for common agent prompts and the distinct neutral Architect templates.
