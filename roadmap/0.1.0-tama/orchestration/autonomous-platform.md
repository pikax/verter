# Autonomous execution platform

The TAMA DAG can be drained autonomously by the external controller/worker platform (the `verter-dag-manager` workspace). This document records the contract between that platform and this repository's authority. The platform is operational tooling: nothing here weakens the authority rules in `contracts/` and `APPLICATION.md`, which always win on conflict.

## Authority boundaries

- DAG modules, charters, catalogs define work. `authority/state/implemented.toml` (schema 2, predeclared — see `decisions/2026-09-01-predeclared-implementation-ledger.md`) defines completion by row status. GitHub branches and PRs are durable coordination and landing evidence.
- The controller's runtime database is operational and reconstructable only. Deleting it never changes what is implemented or READY; on restart the controller re-reads TAMA authority, remote claims, and PRs, and asks reconnecting workers for live sessions.
- Readiness is computed exclusively by `tools/lib.mjs` (`deriveState`); the controller imports it and never reimplements the DAG.

## Execution unit and claims

One DAG node → one claim → one branch → one worktree → one PR → one squash landing. The claim branch is `tama_dag/<NODE_ID>` (no issue/machine/agent ids in branch names); its remote existence is the durable claim. Train-wide branches and stacked PRs are not the normal workflow — the DAG already represents dependency stacking. A draft PR opens after the first pushed implementation commit and stays the same PR through review, repair, and landing (via `scripts/githubctl` composition).

## Scheduling

TAMA readiness is a precondition; the scheduler additionally enforces, as admission constraints (never DAG edges): exclusive conflict-domain leases (two READY nodes sharing a domain never mutate concurrently), resource-class capacity hints from `catalogs/resource-profiles.toml`, worker mutating-slot capacity, external requirements, and a global active-node cap. Runnable nodes rank deterministically: operator boost, critical path, downstream unlock count, release gating, milestone, size — node id only as the final tie-break.

## Sizing

LOC/file/package numbers remain planning guidance and dashboard telemetry (`contracts/sizing.md`). The platform never mechanically rescopes, splits, rejects, or stops a node for crossing a numeric budget, and never splits a node to satisfy an external reviewer's file limit.

## Reviews, CI, CodeRabbit

Each node's own review profile (`catalogs/review-profiles.toml`) drives reviewer count and lenses; reviewers are fresh, independent, read-only; high-risk profiles diversify provider families. Findings consolidate once per round into one fix owner; two review/fix cycles are the soft maximum before model-tier escalation. Local verification is targeted; the canonical merge-authoritative verification is CI-owned — the stable aggregate check is **`CI Required`** (statically enforced complete by `scripts/check-ci-aggregate.mjs`). Red CI is triaged (REAL / FLAKY / INTERACTION / INCONCLUSIVE, per the gate-triage model) before any fix is attempted. CodeRabbit is supplemental external review read structurally from PR reviews/comments: findings merge into the fix workflow; unavailability blocks landing unless an operator records an explicit override.

## Landing

Only the controller lands autonomous work, one candidate at a time: acquire slot → fetch newest main → mechanical rebase (ledger-only conflicts merged deterministically by `tools/merge-ledger.mjs`, registered as the `tama-ledger` git merge driver in `.gitattributes`) → semantic conflicts dispatch an integration agent and invalidate affected reviews → fresh required CI against the current base → verify reviews/conversations/external review → squash merge → post-merge main verification. A PR green against an older main is never merged without revalidation.

## Main circuit breaker

Red main immediately stops new claims and landings without destroying branch-local work, and dispatches a frontier-tier recovery investigation (failing lanes, previous green, recent landings, deterministic/flaky/interaction) that chooses the smallest correct repair or revert, landed through the same PR path. Green main reconciles state, recomputes the frontier, and resumes.

## GitHub protection

`githubctl protection --check` (default) reports drift against the versioned expectation in `scripts/githubctl/protection-expected.json`: active ruleset on the default branch, PR required, force pushes and deletion blocked, linear history, squash as the merge method, conversation resolution, `CI Required` as the required status check, strict up-to-date policy, and zero GitHub-native human approvals (TAMA supplies its own reviewer authority). `--apply` (doctor-gated, admin capability) repairs drift; it never deletes or edits differently-named rulesets. No GitHub auto-merge: the controller is the only landing scheduler.

## Operating

See the `verter-dag-manager` workspace README for controller/worker/web setup, operator scopes (`view/control/steer/dispatch/land/admin`), and the audited operator surface (pause/resume scheduling, prioritize, steer/interrupt per honest harness capability, cancel, retry, escalate, CodeRabbit override, landing stop, main recovery).
