---
name: multi-agent-orchestration
description: >-
  Drive a substantial implementation or migration through bounded implementation,
  risk-scaled fresh review, consolidated fixes, verification, landing, and cleanup.
---

# Multi-Agent Orchestration

Use one parent orchestrator to own ordering, scope, review, landing, and cross-train coordination. A train manager owns only its named train. Use subagents for concrete bounded implementation or fresh review work when the task calls for multi-agent execution.

## Tama readiness and ledger

Tama readiness is intentionally simple. `roadmap/0.1.0-tama/authority/state/implemented.toml` is the only implementation ledger. A row means the node is implemented. A dispatchable node is READY when every transitive DAG ancestor has a row; no activation, conditional, or in-progress state participates.

Each row records the node ID, a useful commit-message search phrase, an approximate timezone-bearing date, and optionally a PR number. Those values are locator hints only. Never resolve or validate them, require exact matching, compare Git SHAs or trees, inspect ancestry, contact GitHub, or create a parallel receipt database.

The implementation patch adds its ledger row before squash or review, using the planned squash message and approximate date. Then squash once and review the resulting candidate. No post-commit ledger update is required.

After the GitHub train lands, issue identity comes from separate `[[github_issue]]` rows in the same ledger file. They map `node_id` to `gh_issue`, require `sync_to_github`, and never mark implementation complete. `true` permits one-way DAG/charter refresh; `false` protects a pre-existing issue from every agent-driven GitHub mutation. Use `githubctl sync-issues` for the initial mapping, explicit later train additions, or an explicit refresh after a block rescope/content change. A refresh updates only an opt-in issue and preserves its number and discussion; protected issues are reported and skipped, and GitHub edits never flow back. Do not put DAG metadata or effort into issues. The implementing agent creates the PR with the expected final title and links the mapped issue. Only for `sync_to_github = true` does it write the useful description and final `Model: <model name>` line; it never edits a protected issue. The finishing agent records the known PR number in the same-patch implementation row before squash and final review.

If an existing GitHub issue must become DAG work, a maintainer manually authors the DAG node, charter, and `[[github_issue]]` row with `sync_to_github = false` in one reviewed patch. Never rewrite that issue or generate, propose, import, or apply DAG authority from GitHub automatically.

## Admission and scope

Before implementation, confirm the node is READY, read its packet and charter, enumerate independently landable outcomes, and select proportionate evidence for every acceptance outcome. Split work that combines unrelated authority changes or independently rollbackable concerns. Tests are evidence, not quota; behavioral changes use TDD.

Conflict domains, resources, external requirements, and effort fields are planning instructions. They are not leases or machine-validated authorizations. The maintainer coordinates ownership and ordering.

## Implementation and worktrees

Use one branch/worktree for the train. Implement only the authorized scope, add the implementation-ledger row to the patch, run targeted evidence, rebase as needed, and squash to one conventional commit before final review. Do not add receipt files, candidate manifests, runtime state, or SHA-bound evidence.

In a fresh worktree, run `pnpm install --frozen-lockfile` before JS/TS tests or workspace-importing Node scripts so missing gitignored dependencies do not look like regressions.

Reviewers should inspect a stable candidate patch. The trust model does not require machine enforcement of immutability. Any material fix invalidates affected review conclusions by judgment; rerun the relevant review and verification without restamping identities.

## Risk-scaled review

- Low/simple: one fresh adversarial reviewer.
- Medium: adversarial plus an optional conformance lens when the profile calls for it.
- High/critical: three fresh tasks—adversarial, conformance, and a context-specific specialist.

Reviewers inspect the cumulative patch, proof selection, applicable tests, scope completeness, fail-closed behavior, performance implications, and architecture conformance. The author does not review its own work.

Consolidate all findings once per round. One fix agent addresses the full set and class-wide siblings. Add a regression only for a plausible boundary not already discriminated. Two review/fix cycles are the soft maximum; use a neutral Architect only for real unresolved architecture ambiguity or a justified continuation ruling.

## Verification, landing, and cleanup

Run targeted evidence during implementation and the owning final gate on the final candidate. Land through the normal repository workflow. There is no fast-forward identity, landing receipt, activation command, or confirmation manifest.

Remove disposable worktrees after their results are recorded. Report the implemented node, commit locator hints, review verdicts, verification results, remaining limitations, and cleanup state.

See `references/templates.md` for prompts.
