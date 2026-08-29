<!-- unified-charter-v2
id=REL2
name=Release PR, tag and publication integration
predecessors=REL1
phase=governance
train=governance.release-control
product=release_control
kind=implementation
semantic_role=delivery
class=successor
owner=governance.release-control:authorized release PR and exact existing tag-publication workflow integration
conflict_domains=release_orchestration,github_projection_state
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
charter=charters/governance-release-control/REL2.md
size=M
max_production_loc=1000
max_production_files=10
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# REL2 — Release PR, tag and publication integration

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Integrate an authorized dedicated release worktree/branch, post-first-push draft release PR, rehearsal, reviewed GitHub-PR squash merge, the required release commit subject, and existing tag/publication workflows. Never land the release locally first and mirror it afterward.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.github/workflows`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: `ReleaseCutAuthorization`, `ReleasePullRequest`, `ReleaseLanding`, exact `release: v<version>` subject, existing tag/publish workflows.

## Exact predecessor contracts

- **REL1:** implemented ledger row for “Milestone release readiness and release rehearsal”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- After explicit authorization and before mutation, create the dedicated release/version worktree and branch. After the first release/version commit is pushed, open the draft release PR, keep it as the reviewed landing candidate, and run the existing rehearsal.
- Require maintainer landing authorization and passing review/verification; do not add Git-identity validation or landing receipts.
- Squash-merge the reviewed release PR through GitHub; never land locally first and mirror it afterward.
- Preserve the exact squash subject `release: v<version>` without an appended PR suffix when required by `.github/workflows/release-tag.yml`.
- Reuse `.github/workflows/release-tag.yml` and `.github/workflows/release.yml`; preserve alpha/beta/rc prerelease behavior.
- Do not automatically close the milestone without explicit maintainer policy.
- P0/P1 and policy-blocking live carry-forward obligations block release; mutable GitHub issue/milestone state cannot erase them.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **REL2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **REL2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **REL2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **REL2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject a duplicate tag/publish implementation, nonexact release subject, automatic milestone closure, stale rehearsal reuse, implementer self-merge, and mutable finding waiver.
- Do not change prerelease classification or publishing implementation outside the existing workflows.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 10 production files, 3 related packages.
- Rescope if existing tag/publication workflows cannot be reused or a second release authority appears.
- Correctness budget: zero unauthorized release, wrong version/tag/subject, stale workflow evidence, duplicate publication, or finding loss.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on missing predecessor rows, a failed rehearsal, absent maintainer authorization, wrong subject, open P0/P1, or a policy-blocking lower finding.
- Abort rather than rewrite existing release architecture opportunistically.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy.
