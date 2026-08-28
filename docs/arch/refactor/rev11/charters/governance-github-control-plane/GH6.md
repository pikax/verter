<!-- unified-charter-v2
id=GH6
name=GitHub control-plane convergence and cutover
predecessors=GH2,GH5,FB2,REL2
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=convergence
semantic_role=convergence
class=successor
owner=governance.github-control-plane:end-to-end cutover proof over projection, feedback, PR, CI, and release control
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
source_refs=source:github-control-plane-program.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/governance-github-control-plane/GH6.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH6 — GitHub control-plane convergence and cutover

Authority state is derived at dispatch. Cutover requires the current accepted ORC0 activation receipt and all four exact predecessor receipts on one cumulative candidate.

## Independently acceptable outcome

Prove and activate the complete GitHub operational control plane without adding another feature train. Pre-scope result: this node contains only bounded integration fixtures, negative controls, and cutover wiring over already-landed owners; any new projection, feedback, evidence, or release behavior returns to its owner.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.github`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundary: one end-to-end `githubctl` workflow from READY frontier through issue/PR/CI/landed proof plus feedback promotion and release rehearsal.

## Exact predecessor contracts

- **GH2:** exact current receipt ID and digest for “DAG → GitHub issue projection and reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **GH5:** exact current receipt ID and digest for “CI integration gate, safe squash landing and landed proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **FB2:** exact current receipt ID and digest for “Maintainer-authorized issue → DAG promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **REL2:** exact current receipt ID and digest for “Release PR, tag and publication integration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Prove deterministic projection, repeated no-op, exact hierarchy/topology, human-field preservation, and dangerous-drift refusal.
- Prove ignored/non-DAG feedback behavior, unauthorized promotion refusal, ratified promotion issue reuse, and label-only AI mutation.
- Prove draft dispatch, PR-head invalidation, immutable review binding, stale-CI refusal, exact squash-tree landing, and receipt-only DAG advancement.
- Prove P0/P1 refusal, lower-finding carry-forward creation, repeat authorization escalation, immutable resolution supersession, and immunity to mutable GitHub issue changes.
- Prove milestone priority never bypasses READY, blocked release refusal, exact existing rehearsal, and compatible release tag/publication flow.
- Cut over operational usage only after all planted RED/GREEN controls pass on the same frozen candidate.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH6-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject any superseded non-GitHub operational projection named by the GH0 cutover inventory, but only after exact replacement proof on this candidate.
- Delete or structurally reject dual orchestration, mutable GitHub acceptance, implicit finding loss, a duplicate release pipeline, or a compatibility synchronization layer.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related package.
- Any new feature behavior, schema family, or owner exceeds convergence scope and returns to GH/FB/REL authority.
- Correctness budget: zero projection divergence, lost finding, stale evidence, unauthorized promotion/merge/release, or human-field loss.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on any failed owner receipt, stale activation, P0/P1, unexplained lower finding, missing negative control, post-review change, or inability to prove exact landed/release trees.
- Abort rather than accepting partial control-plane activation.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports plus independent full confirmation. P0/P1 block. No carry-forward can disappear through mutable GitHub state; repeated carry-forward requires explicit escalating authorization and final resolution requires immutable supersession.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH6-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:617-651`
- Applicability: `GH6`
- Exact text SHA-256: `2ee0840010453e6938b2440b7622924bc4ad3310cf61df50391376b88a9589c3`

~~~~markdown
## GH6 — GitHub control-plane convergence and cutover

Predecessors:

* GH2
* GH5
* FB2
* REL2

This is the integration/convergence block, not another feature train.

Prove end-to-end:

1. static DAG projects deterministically into GitHub;
2. repeated reconciliation is a no-op;
3. train → block hierarchy is correct;
4. predecessor → blocked-by topology is correct;
5. maintainer-owned milestone/human text survives reconciliation;
6. non-DAG AI inspection is label-only on GitHub and produces the local report;
7. `ai:ignore` prevents mutation;
8. promotion without maintainer/amendment authorization fails;
9. approved promotion reuses the existing issue correctly;
10. dispatch creates/binds a draft PR;
11. PR head changes invalidate frozen evidence;
12. Ready-for-review binds an immutable candidate snapshot;
13. stale CI cannot authorize landing;
14. squash landing verifies exact validated integration content;
15. GitHub closure/labels cannot forge DAG acceptance;
16. release milestone never bypasses DAG readiness;
17. release rehearsal executes the existing exact release graph;
18. release PR integrates correctly with existing tag/publish workflows;
19. human-authored body sections are never overwritten;
20. duplicate or conflicting GitHub/DAG mappings fail closed.

Include planted RED/GREEN negative controls for the important authority boundaries.
~~~~

### SRC-GITHUB-REQUIRED-TESTS

- Kind: `acceptance`
- Source: `github-control-plane-program.md:750-781`
- Applicability: `GH6`
- Exact text SHA-256: `c6d6a3a2e86edcc32d09054ae55bce9f91c10f5ac1acbb905f29ee86a9519e0f`

~~~~markdown
# Required tests

Do not rely on live GitHub for tests.

Create fixtures/fake adapter tests for at least:

* empty project bootstrap;
* existing correct projection;
* partially missing projection;
* duplicate DAG marker;
* human-edited body;
* human milestone movement;
* unrelated labels;
* stale/incorrect DAG lifecycle label;
* issue manually closed without landed receipt;
* non-DAG triage;
* ignored non-DAG issue;
* conflicting AI labels;
* authorized/unauthorized promotion;
* PR head update after candidate freeze;
* stale CI run;
* base movement before merge;
* squash result tree mismatch;
* interrupted sync followed by retry;
* repeated sync no-op;
* milestone release with blocked item;
* milestone release fully ready;
* beta release version/tag path.

Live GitHub smoke testing must be bounded and reversible.

Do not spam the real repository with throwaway issues. If live mutation is necessary, use a clearly named temporary fixture and clean it up only with explicit maintainer authorization.
~~~~

### SRC-GITHUB-END-STATE

- Kind: `acceptance`
- Source: `github-control-plane-program.md:819-891`
- Applicability: `GH6`
- Exact text SHA-256: `afe1529737e337797a1c38ac371ffa30cce47c1dbf17f91a252b83c1341232fe`

~~~~markdown
# End state

The desired operating experience is approximately:

```
maintainer creates/plans milestones
               │
               ▼
      GitHub Project 3
               │
      train / block issues
               │
           labels
               │
    programctl READY frontier
               │
               ▼
    agent picks admitted block
               │
        draft GitHub PR
               │
   implementation/review/fix
               │
        Ready for review
               │
   formal review + GitHub CI
               │
     immutable gate evidence
               │
   orchestrator squash-merge
               │
      landed-tree proof
               │
     successor becomes READY
```

Separately:

```
non-DAG issue
    ↓
AI inspect
    ↓
.feedback/issues/N.md
    ↓
AI result label only
    ↓
maintainer decides
    ↓
optional authorized DAG promotion
```

And release:

```
milestone
   ↓
READY work completed
   ↓
release planner
   ↓
existing release rehearsal
   ↓
release PR
   ↓
squash `release: vX`
   ↓
existing tag workflow
   ↓
existing release workflow
```

The final system should make GitHub pleasant enough to act as the day-to-day orchestration/history surface while retaining ORC0's stronger local/static evidence guarantees underneath it.
~~~~
