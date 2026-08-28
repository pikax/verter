<!-- unified-charter-v2
id=REL2
name=Release PR, tag and publication integration
predecessors=REL1
conditional_predecessors=
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
source_refs=source:github-control-plane-program.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/governance-release-control/REL2.md
size=M
max_production_loc=1000
max_production_files=10
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# REL2 — Release PR, tag and publication integration

Authority state is derived at dispatch. Release execution requires the current accepted ORC0 activation receipt, current REL1 readiness/rehearsal receipt, and explicit maintainer release-cut authorization.

## Independently acceptable outcome

Integrate authorized release branch/PR creation, exact rehearsal, orchestrator-approved squash, exact release commit subject, and existing tag/publication workflows. Pre-scope result: these steps are one release transaction because partial landing without compatible tag/publication proof is not independently acceptable.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.github/workflows`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `ReleaseCutAuthorization`, `ReleasePullRequest`, `ReleaseLanding`, exact `release: v<version>` subject, existing tag/publish workflows.

## Exact predecessor contracts

- **REL1:** exact current receipt ID and digest for “Milestone release readiness and release rehearsal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- After explicit authorization, create the release/version branch and draft release PR, bind exact head/base/tree, and run the existing rehearsal.
- Require maintainer/program-orchestrator landing authorization, expected-head protection, current evidence, and exact squash tree proof.
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
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject a duplicate tag/publish implementation, nonexact release subject, automatic milestone closure, stale rehearsal reuse, implementer self-merge, and mutable finding waiver.
- Do not change prerelease classification or publishing implementation outside the existing workflows.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 10 production files, 3 related packages.
- Rescope if existing tag/publication workflows cannot be reused or a second release authority appears.
- Correctness budget: zero unauthorized release, wrong version/tag/subject/tree, stale workflow evidence, duplicate publication, or finding loss.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation/readiness/rehearsal, absent maintainer authorization, head/base movement, wrong subject, tree mismatch, open P0/P1, or policy-blocking carry-forward.
- Abort rather than rewrite existing release architecture opportunistically.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Lower-severity carry-forward remains immutable and issue-bound; repeated carry-forward requires escalation and mutable GitHub state cannot authorize release by erasing it.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-REL2-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:570-615`
- Applicability: `REL2`
- Exact text SHA-256: `a512ce0df15c99ba64886672f2183a46acb61e6bc088468c288358a4af396016`

~~~~markdown
REL2 — Release PR, tag and publication integration
Predecessor: REL1

After the maintainer explicitly authorizes a release cut:

```
milestone ready
     ↓
create release/version-bump branch
     ↓
draft release PR
     ↓
run exact release rehearsal
     ↓
validate
     ↓
maintainer/program-orchestrator authorizes landing
     ↓
squash merge
     ↓
exact release commit subject
     ↓
existing release-tag.yml
     ↓
existing release.yml
```

The squash commit subject must remain compatible with current release-tag.yml, currently:

```
release: v<version>
```

Example:

```
release: v0.1.0-beta.5
```

Do not append `(#PR)` if that would break the current exact matcher.

Use the existing release tagging/publishing implementation rather than replacing it.

Alpha/beta/rc releases remain prereleases according to the existing `release.yml` contract.

Do not automatically close the milestone unless the maintainer explicitly chooses that policy.
~~~~
