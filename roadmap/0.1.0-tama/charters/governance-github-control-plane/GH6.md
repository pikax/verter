<!-- unified-charter-v2
id=GH6
name=Minimal GitHub workflow convergence and cutover
predecessors=GH2,GH5,FB2,REL2
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=convergence
semantic_role=convergence
class=successor
owner=governance.github-control-plane:end-to-end proof of issue mapping, PR, CI, and release coordination
conflict_domains=github_projection_state,feedback_operations,release_orchestration
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
charter=charters/governance-github-control-plane/GH6.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH6 — Minimal GitHub workflow convergence and cutover

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


## Independently acceptable outcome

Prove and activate the minimal GitHub workflow without adding another feature train. This node contains bounded integration fixtures and cutover wiring over already-landed owners; it must leave `githubctl sync-issues` available for the initial mapping run, occasional later train additions, and explicit mapped-issue refresh after a block rescope/content change.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.github`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundary: one end-to-end workflow from ancestor-row READY through local issue mapping, dedicated node worktree/branch before mutation, first pushed implementation commit, final-title draft PR, issue description, CI, ledger finalization, reviewed GitHub-PR squash merge, feedback, and release rehearsal. Local-first landing/mirroring is forbidden.

## Exact predecessor contracts

- **GH2:** implemented ledger row for “Occasional issue-sync command and local mapping”; ledger presence alone satisfies the predecessor and its commit metadata remains an unvalidated locator.
- **GH5:** implemented ledger row for “CI, ledger finalization and squash landing”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **FB2:** implemented ledger row for “Maintainer-authored DAG block and issue mapping”; ledger presence alone satisfies the predecessor and its commit metadata remains an unvalidated locator.
- **REL2:** implemented ledger row for “Release PR, tag and publication integration”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Prove one-way explicit-scope issue creation and in-place opt-in issue update from local authority, strict refusal to rewrite `sync_to_github = false` issues, unique local `gh_issue` mappings, preserved issue number/discussion, lookup by issue number without reverse synchronization, clear partial-failure reporting, and no accidental implementation completion.
- Prove opt-in issue bodies contain useful human description plus only the final model line as workflow metadata—no effort or DAG block—and protected issue bodies are never edited.
- Prove mapping plus dedicated worktree/branch before mutation, draft final-title PR creation only after the first implementation commit is pushed, the exact mapped `Closes #<gh_issue>` link, issue closure on merge, readable review reporting, unexpected-CI-skip refusal, finishing-agent ledger update, reviewed GitHub-PR squash merge, no local-first mirroring, and no post-merge restamping.
- Prove P0/P1 refusal, owning-policy handling for lower findings, and that GitHub issues can enter the DAG only through a manually authored DAG/charter/mapping patch.
- Prove milestone priority never bypasses READY, blocked release refusal, and compatible release tag/publication flow.
- Cut over operational usage only after the applicable RED/GREEN controls and owning final gate pass on the squashed review candidate.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH6-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject machine markers, managed issue regions, DAG/effort metadata, topology projection, continuous reconciliation, GitHub-derived completion, implicit finding loss, or a duplicate release pipeline.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related package.
- Any new feature behavior, schema family, or owner exceeds convergence scope and returns to GH/FB/REL authority.
- Correctness budget: zero duplicate mapping, accidental completion, lost finding, stale evidence, automatic issue-to-DAG mutation, unauthorized merge/release, or generated metadata in issues.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on a missing predecessor ledger row, P0/P1, unexplained lower finding, missing negative control, or failed owning gate.
- Abort rather than accepting a partial control-plane cutover.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports plus independent full confirmation. P0/P1 block; lower findings follow the owning review policy.
