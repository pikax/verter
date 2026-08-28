<!-- unified-charter-v2
id=GH5
name=CI integration gate, safe squash landing and landed proof
predecessors=GH4
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:receipt-bound CI integration proof and orchestrator-only squash landing
conflict_domains=github_projection_state,release_orchestration
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
charter=charters/governance-github-control-plane/GH5.md
size=M
max_production_loc=1200
max_production_files=12
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH5 — CI integration gate, safe squash landing and landed proof

Authority state is derived at dispatch. CI import and merge require the current accepted ORC0 activation receipt and current GH4 PR/evidence bridge receipt.

## Independently acceptable outcome

Make GitHub CI the integration-gate executor while retaining Rev11 evidence authority, then add orchestrator-only expected-head squash landing and exact landed-tree proof. Pre-scope result: CI import, merge admission, and landed receipt are one atomic acceptance transaction because no partial subset may advance lifecycle state.

## Concrete surfaces and APIs

- Production surfaces: `.github`, `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `CiEvidenceImport`, `ValidatedIntegration`, `MergeAdmission`, `ExpectedHead`, `LandedTreeProof`, generic landed receipt.

## Exact predecessor contracts

- **GH4:** exact current receipt ID and digest for “PR body, review history and immutable evidence bridge”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Import CI evidence bound to node, PR, head, integration base, tested integration tree, exact commands, unexpected skips, and terminal result through existing gate primitives.
- Refuse GitHub-green as authority without complete immutable evidence; stale/missing jobs or skipped work fail closed.
- Freeze expected PR/head and integration-base policy before merge; movement invalidates and reruns rather than rebasing or guessing.
- Permit only the program orchestrator to squash-merge DAG PRs, using expected-head protection.
- Fetch the resulting main commit and require `landed_tree == validated_integration_tree`; emit landed receipt before DAG state or `dag:landed` changes.
- Acceptance refuses P0/P1 and refuses lower actionable findings without valid policy-owned non-actionable acceptance or live immutable carry-forward. Mutable GitHub state cannot erase the obligation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH5-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject merge-button authority, `candidate_sha == landed_sha` as the invariant, automatic close keywords as correctness, stale check reuse, implementer self-merge, and state advance before landed proof.
- Do not replace existing Rev11 gate/evidence schemas with GitHub check conclusions.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 12 production files, 3 related packages.
- Rescope if landing cannot be one fail-closed transaction or if a second evidence system is proposed.
- Correctness budget: zero stale merge, wrong tree, skipped evidence, unauthorized merger, lost finding, or premature lifecycle advance.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation, head/base movement, incomplete review/finding disposition, missing CI job, unexpected skip, merge identity mismatch, or landed-tree mismatch.
- Abort if expected-head protection or fetched-tree proof is unavailable.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Lower carry-forward must remain live in immutable receipts and a durable issue; mutable GitHub closure cannot make the merge admissible.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH5-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:271-334`
- Applicability: `GH5`
- Exact text SHA-256: `4c449b679066cde8750208d206e68b793e412db59a0f97e31f3ca5027030ff91`

~~~~markdown
GH5 — CI integration gate, safe squash landing and landed proof
Predecessor: GH4

Make GitHub CI the top-level integration gate executor for a block.

Do NOT replace the Rev11 evidence schema with "GitHub says green".

The required CI job must emit/import evidence bound to:

* DAG node;
* PR;
* PR head;
* exact integration base;
* exact tested integration tree;
* commands actually executed;
* unexpected skips;
* terminal result.

Use existing `programctl gate-run`/evidence primitives where possible.

The final invariant for squash merging is:

```
landed_tree == validated_integration_tree
```

NOT:

```
landed_sha == candidate_sha
```

Only the program orchestrator may squash-merge DAG block PRs.

Before merge verify:

* PR is the expected PR;
* current head equals the frozen head;
* required review evidence is current;
* required CI/gate evidence is current;
* tested integration base is still current enough under the defined policy.

If head or required integration base changed, invalidate/rerun rather than guessing.

Use expected-head protection (`--match-head-commit` or equivalent).

After squash:

* fetch the resulting main commit;
* validate landed tree against the accepted integration tree;
* emit a generic landed receipt;
* only then mark the DAG node LANDED / satisfy successors.

One DAG block should normally become one squash commit on `main`.

The detailed intermediate history belongs in its PR.

Do not use automatic `Closes #N` as correctness authority.

Use `Refs #N`.

For generated DAG issues, initially make issue auto-closing configurable and default conservatively. A `dag:landed` state must always come from verified landed evidence regardless of whether the human issue remains open.

## governance.feedback-intake
~~~~
