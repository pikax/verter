<!-- unified-charter-v2
id=GH1
name=GitHub adapter and deterministic fixtures
predecessors=GH0
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:replaceable structured GitHub adapter and deterministic fake
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
charter=charters/governance-github-control-plane/GH1.md
size=M
max_production_loc=900
max_production_files=10
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH1 — GitHub adapter and deterministic fixtures

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


## Independently acceptable outcome

Deliver a small structured GitHub adapter, permission check, and deterministic fake for explicit issue creation, in-place opt-in issue updates, protected-issue refusal, and PR operations. The adapter and fake land together; the occasional issue-sync command remains GH2.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: `GitHubAdapter`, `GitHubDoctor`, `FakeGitHubAdapter`, issue creation, issue update, and PR creation records.

## Exact predecessor contracts

- **GH0:** implemented ledger row for “Minimal GitHub workflow and local issue mapping”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Keep `programctl` local and read-only with respect to GitHub; put all network effects behind `GitHubAdapter`.
- Consume supported structured `gh issue`, `gh pr`, and `gh api` output; never scrape terminal or UI prose.
- Doctor validates auth, repository access, and the exact issue/PR mutation capabilities needed before a write.
- The deterministic fake models issue creation, opt-in title/body update with number/history preservation, protected-mapping write refusal, PR creation with an exact mapped-issue closing link, returned numbers, permissions, and partial failure without live GitHub.
- Do not add Project-field, label, milestone, blocker-topology, managed-body, or DAG-projection machinery.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject direct GitHub calls from `programctl`, unstructured stdout parsing, credential persistence, implicit live-test dependencies, and mutation APIs without check mode.
- Do not implement issue synchronization, feedback semantics, release planning, or merge authorization in this block.

## Budgets and mandatory rescope

- Target ceiling: 900 production LOC, 10 production files, 2 related packages.
- Rescope on a third transport implementation or direct lifecycle-kernel coupling.
- Correctness budget: zero ambiguous mutation, token persistence, prose scraping, or fake/live semantic divergence for the small supported surface.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if current `gh` structured capabilities cannot return issue and PR numbers safely or if live discovery would be needed in tests.
- Abort on a missing GH0 ledger row, missing permission, wrong repository, or nondeterministic fake behavior.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower-severity findings follow the owning review policy and may be tracked in GitHub.
