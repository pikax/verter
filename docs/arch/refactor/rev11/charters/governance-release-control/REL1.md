<!-- unified-charter-v2
id=REL1
name=Milestone release readiness and release rehearsal
predecessors=REL0,GH5
conditional_predecessors=
phase=governance
train=governance.release-control
product=release_control
kind=implementation
semantic_role=delivery
class=successor
owner=governance.release-control:receipt-derived milestone readiness and exact existing release rehearsal
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
charter=charters/governance-release-control/REL1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# REL1 — Milestone release readiness and release rehearsal

Authority state is derived at dispatch. Release planning requires the current accepted ORC0 activation receipt plus current REL0 and GH5 receipts.

## Independently acceptable outcome

Implement a milestone release planner whose DAG-managed readiness comes exclusively from accepted/landed Rev11 evidence and whose rehearsal invokes the existing release workflow graph. Pre-scope result: readiness report and exact dry-run rehearsal are atomic; release PR/tag/publication remain REL2.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.github/workflows`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `release plan <milestone>`, `ReleaseReadiness`, `ReleaseBlocker`, exact `release-check.yml` rehearsal invocation.

## Exact predecessor contracts

- **REL0:** exact current receipt ID and digest for “Milestone release-planning overlay and DAG scheduling”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **GH5:** exact current receipt ID and digest for “CI integration gate, safe squash landing and landed proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Inspect all milestone items and derive DAG-managed readiness from immutable receipts, not issue closure, labels, Project status, or milestone progress.
- Report every blocking node/item and exact missing predecessor/evidence without silent waiver.
- Permit only maintainers to move, defer, or waive release content.
- Reuse `.github/workflows/release-check.yml` and its exact dry-run invocation of `.github/workflows/release.yml`; do not create a duplicate release validator.
- Bind rehearsal identity, inputs, executed workflow/job set, skips, and terminal result into release planning evidence.
- Open carry-forward obligations remain live release inputs according to owning policy; mutable GitHub closure cannot erase them, and P0/P1 remain blockers.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **REL1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **REL1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **REL1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **REL1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject issue-closure readiness, silent waiver, AI milestone movement, duplicate release workflow, skipped-job success, and mutable finding resolution.
- Do not create/merge the release PR, tag, publish, or close the milestone in this block.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related packages.
- Rescope if readiness needs a second lifecycle/evidence database or a replacement release workflow.
- Correctness budget: zero false-ready release, hidden blocker, stale rehearsal, unexpected skip, or finding loss.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation/items/receipts, missing required workflow, unexpected skip, ambiguous waiver, open P0/P1, or policy-blocking carry-forward.
- Abort rather than infer maintainer release intent.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Carry-forward release treatment is receipt/policy-owned and cannot be changed by mutable GitHub issue, label, milestone, or closure state.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-REL1-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:547-568`
- Applicability: `REL1`
- Exact text SHA-256: `7ab6db8c467141f62cacafee5319b3bf7b9b42a59531c50aa3495559e2c77b23`

~~~~markdown
REL1 — Milestone release readiness and release rehearsal
Predecessors: REL0, GH5

Create a release planner that can inspect a milestone and determine whether a release may be cut.

Conceptual interface:

```
githubctl release plan 0.1.0-beta.5
```

For DAG-managed issues, readiness must derive from accepted/landed Rev11 evidence, not merely GitHub closure.

If a required milestone item is not ready, report exactly what blocks the release.

Do not silently waive work.
Only the maintainer can move/defer/waive release content.

Reuse the existing `.github/workflows/release-check.yml`.

It intentionally invokes the exact `release.yml` graph with `dry_run: true`.
Preserve that architecture; do not build a duplicate release validation pipeline.
~~~~
