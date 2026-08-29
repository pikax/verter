<!-- unified-charter-v2
id=GH0
name=Minimal GitHub workflow and local issue mapping
predecessors=ORC0
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=constitution
semantic_role=delivery
class=successor
owner=governance.github-control-plane:minimal GitHub workflow and local ledger mappings
conflict_domains=github_projection_state,feedback_operations,release_orchestration
resource_class=docs-light
gate_profile=docs-domain
review_profile=security-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/governance-github-control-plane/GH0.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH0 — Minimal GitHub workflow and local issue mapping

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


The maintainer ruling and rationale are preserved in `../../decisions/2026-08-28-minimal-github-issue-mapping.md`.

## Independently acceptable outcome

Ratify the minimal GitHub workflow without implementing synchronization. The local ledger maps nodes to issue numbers; GitHub holds ordinary human descriptions, PRs, reviews, and CI. No DAG metadata is serialized into issues, and adapter code remains GH1.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama/contracts/github-control-plane.md`, `roadmap/0.1.0-tama/schemas/implementation-ledger.schema.json`, and `roadmap/0.1.0-tama/authority/state/implemented.toml`.
- Named boundaries: `GitHubIssueMapping`, `GitHubIssueDescription`, `ExpectedPullRequestTitle`, and `GitHubIssueSync`.
- Mutation boundary: authority and schema bytes only; no `gh`, network, issue, PR, Project, label, milestone, or workflow mutation.

## Exact predecessor contracts

- **ORC0:** implemented ledger row for “Trusted implementation-ledger cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Define separate `[[github_issue]]` rows containing `node_id`, `gh_issue`, and required `sync_to_github`. `true` permits one-way refresh; `false` protects a pre-existing issue. Neither value marks implementation complete.
- Keep opt-in issue title/body ordinary and human-readable. Synthesize a standalone `Problem`, `Expected outcome`, and three-to-six-bullet `Acceptance` body from the charter and current source; do not copy charter sections, program/DAG wording, abort conditions, budgets, gates, commands, or generic boilerplate. End with `Model: <model name>`. Protected issues remain untouched.
- Define the agent flow when GitHub control is active: resolve the mapping and create the independently landable node's dedicated worktree/branch before mutation; after the first implementation commit is pushed, open the draft PR with the expected final conventional-commit title and exact `Closes #<gh_issue>` body link. Keep that reviewed PR as the landing candidate and squash-merge it through GitHub; never land locally first and mirror it afterward. Update the useful issue description only when `sync_to_github = true`; never edit a protected issue.
- Define the finishing flow: before squash/review completion, update the `[[implemented]]` row with message, approximate date, and known PR number.
- Define an occasional post-train `githubctl sync-issues` command for initial issue creation, later train additions, and explicit in-place refresh after a block rescope/content change. Content flows only from local DAG/charter authority to an opt-in mapped issue. Preserve the issue number and discussion, refuse protected-issue writes, and never import GitHub edits or continuously reconcile.
- Do not implement `scripts/githubctl` or inspect live repository configuration in this block.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject DAG metadata in issue bodies, hidden markers, managed regions, effort fields, continuous reconciliation, title identity, or GitHub closure satisfying ancestors.
- No legacy production path is deleted by this constitution; GH1–GH6, FB0–FB2, and REL0–REL2 own future implementation and cutover.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related packages.
- Rescope before adding executable adapter logic or live GitHub state.
- Correctness budget: zero issue mapping ambiguity, accidental implementation from an issue mapping, mutable-state authority, or readiness derived outside implementation rows.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if ORC0 lacks a ledger row or a policy requires GitHub state as implementation authority.
- Abort if issue creation cannot immediately record the returned issue number in the local mapping patch.

## Targeted verification

1. `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Apply `security-3`. Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports on the squashed review candidate. P0/P1 block. Every surviving actionable lower-severity finding follows the carry-forward contract; no mutable GitHub issue operation can dispose it.
