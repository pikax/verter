<!-- unified-charter-v2
id=GH3
name=Final-title PR creation and issue description
predecessors=GH2
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:agent-owned PR creation and human issue description
conflict_domains=github_projection_state
resource_class=ts-heavy
gate_profile=canonical
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
charter=charters/governance-github-control-plane/GH3.md
size=M
max_production_loc=1000
max_production_files=10
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH3 — Final-title PR creation and issue description

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


## Independently acceptable outcome

Define the agent-owned start flow: create the PR with the expected final conventional-commit title and link the locally mapped issue using `Closes #<gh_issue>`, so GitHub closes it when the PR merges. Put the useful implementation description on an opt-in issue; leave a protected pre-existing issue untouched. Review history remains GH4.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: `ExpectedPullRequestTitle`, `GitHubIssueMapping`, human issue description, and optional PR locator in the implementation ledger.

## Exact predecessor contracts

- **GH2:** implemented ledger row for “Occasional issue-sync command and local mapping”; ledger presence alone satisfies the predecessor and its commit metadata remains an unvalidated locator.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- After READY, use the local `gh_issue` mapping, create the branch, and create a PR whose title is already the planned final conventional commit message.
- Put exactly the mapped `Closes #<gh_issue>` link in ordinary PR prose. This is required for both mapping policies and closes the issue only on merge. When `sync_to_github = true`, write or refresh the useful implementation description and end it with exactly `Model: <model name>`. When false, do not edit the issue.
- Do not put effort, reasoning tier, DAG ID, predecessors, readiness, generated labels, markers, or metadata blocks in the issue or PR.
- Treat the PR number as a locator and include it in the implementation row when the finishing agent completes the ledger fields.
- Preserve one-node/one-issue/one-PR/one-squash normal policy; exceptions require explicit charter authority.
- Do not infer implementation from PR state or add a second PR-binding database.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject draft-lifecycle metadata, commit-SHA handoff, duplicate PR creation, static DAG mutation on PR creation, and implementation state derived from ready/merged state.
- Do not implement PR review-cycle prose, CI import, or merge execution in this block.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 10 production files, 3 related packages.
- Rescope if the block introduces an identity/receipt database or combines merge authorization.
- Correctness budget: zero duplicate PR, wrong or missing closing link, wrong local issue mapping, protected-issue edit, non-final PR title, or authority mutation.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on a missing ancestor row, missing local issue mapping, ambiguous existing PR, or wrong repository.
- Abort rather than guessing an issue from its title or body.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy.
