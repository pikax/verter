<!-- unified-charter-v2
id=GH3
name=PR-backed dispatch and draft lifecycle
predecessors=GH2
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:runtime DAG-node to draft-PR binding beneath immutable candidate identity
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
source_refs=source:github-control-plane-program.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/governance-github-control-plane/GH3.md
size=M
max_production_loc=1000
max_production_files=10
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH3 — PR-backed dispatch and draft lifecycle

Authority state is derived at dispatch. Draft-PR creation requires the current accepted ORC0 activation receipt and current GH2 projection receipt.

## Independently acceptable outcome

Bind admitted DAG work to an early draft PR while preserving immutable candidate/base/tree authority below the human PR abstraction. Pre-scope result: dispatch-time branch/PR creation and stale-head invalidation are one lifecycle transaction; PR narrative and review history remain GH4.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `PullRequestBinding`, `DispatchPrPlan`, `FrozenPrRevision`, draft-to-ready transition, candidate finalization hook.

## Exact predecessor contracts

- **GH2:** exact current receipt ID and digest for “DAG → GitHub issue projection and reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- After READY, admission, and round-bound ownership, create/select the branch and draft PR, then bind node, issue, PR database identity/number/URL, and repository.
- Treat PR number as operational identity while freezing exact head SHA/tree, base SHA/tree, integration identity, authority digest, charter digest, and lease/dispatch identity beneath it.
- A head change after candidate finalization invalidates the old candidate and all dependent evidence automatically.
- PR creation is runtime state and never mutates static DAG authority or satisfies a predecessor.
- Preserve one-node/one-issue/one-PR/one-squash normal policy; exceptions require explicit charter authority.
- Do not infer finding resolution from PR state; carry-forward remains immutable-receipt authority and mutable GitHub state cannot dispose it.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject manual candidate-SHA handoff as the normal operational identity, duplicate PR creation, static DAG mutation on PR creation, and acceptance derived from ready/merged state.
- Do not implement PR review-cycle prose, CI import, or merge execution in this block.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 10 production files, 3 related packages.
- Rescope if the block changes receipt schemas beyond PR binding/head currency or combines merge authorization.
- Correctness budget: zero duplicate PR, stale-head acceptance, wrong issue binding, or authority mutation.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation, missing issue binding, ambiguous PR, changed candidate basis, or inability to use expected repository/head identity.
- Abort if PR creation cannot be made retry-safe through stable IDs.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. A lower finding carried through PR work remains live by immutable receipt despite mutable GitHub draft, ready, closed, or merged state.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH3-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:165-218`
- Applicability: `GH3`
- Exact text SHA-256: `61808ceeb98047a893c22b8b45de2e3396d7861ceb2ecf8d1ebfbdf54042d641`

~~~~markdown
GH3 — PR-backed dispatch and draft lifecycle
Predecessor: GH2

Change block execution so a dispatched block receives/creates a GitHub draft PR early.

Desired flow:

```
DAG READY
  ↓
admit/lease
  ↓
branch
  ↓
draft PR immediately
  ↓
bind DAG node → GitHub PR
  ↓
implementation/review-fix work
  ↓
final candidate
  ↓
Ready for review
  ↓
formal gate/review
  ↓
squash landing
```

The PR number is the stable operational work identity.

Humans/agents should normally reason in:

```
CCA1 / Issue #N / PR #M
```

rather than manually exchanging candidate SHAs.

However, DO NOT weaken immutable evidence.

Internally, `programctl` must still freeze and bind:

* exact PR head SHA/tree;
* base SHA/tree;
* integration SHA/tree where applicable.

The SHA/tree is an internal immutable snapshot beneath the PR abstraction.

If PR #M changes after candidate finalization, old evidence becomes stale automatically.

Do not mutate static DAG authority merely because a PR was created.

PR bindings are runtime/external-control-plane state.
~~~~

### SRC-GITHUB-PR-POLICY

- Kind: `requirement`
- Source: `github-control-plane-program.md:719-748`
- Applicability: `GH3`, `GH4`, `GH5`
- Exact text SHA-256: `15bd2cb677d2c2ac5ecb4d708ba0e73e5423e610f0d78b48d3e3677e83ec9027`

~~~~markdown
# PR policy

Normal DAG block:

```
one DAG block
  =
one GitHub issue
  =
one implementation PR
  =
one squash commit on main
```

Exceptions require an explicit charter reason.

Draft PR is the normal implementation state.

Only the program orchestrator lands DAG block PRs.

The block agent/implementer may:

* create/push its branch;
* create/update its draft PR;
* update managed PR description;
* execute its assigned implementation/review-fix work.

It may not independently squash-merge itself.

Use `Refs #issue`, not automatic close keywords, unless a future explicit policy changes that.
~~~~
