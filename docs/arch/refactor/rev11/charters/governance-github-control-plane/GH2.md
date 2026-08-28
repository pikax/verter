<!-- unified-charter-v2
id=GH2
name=DAG → GitHub issue projection and reconciliation
predecessors=GH1
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:deterministic issue hierarchy and field-owned reconciliation
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
charter=charters/governance-github-control-plane/GH2.md
size=M
max_production_loc=1200
max_production_files=12
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH2 — DAG → GitHub issue projection and reconciliation

Authority state is derived at dispatch. Reconciliation requires the current accepted ORC0 activation receipt plus current GH1 adapter receipt before reading or writing GitHub.

## Independently acceptable outcome

Project the static DAG into one train issue per train, one node issue/sub-issue per node, and exact blocked-by edges with deterministic, field-owned reconciliation. Pre-scope result: hierarchy, topology, managed content, and lifecycle-label projection are one atomic idempotence boundary; PR dispatch remains GH3.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `DagProjection`, `IssueBinding`, `ManagedRegion`, `ProjectionDrift`, `ReconcilePlan`, `sync --check`, `sync --apply`.

## Exact predecessor contracts

- **GH1:** exact current receipt ID and digest for “GitHub adapter, Project discovery and deterministic fixtures”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Resolve train/node identity only from durable markers, never titles, labels, milestone, or Project row position.
- Generate one parent issue per train, one issue/sub-issue per node, exact parent mapping, Project 3 membership, and predecessor blocked-by topology.
- Preserve maintainer milestone, human text outside the managed region, and labels outside owned namespaces byte-for-byte or set-for-set as appropriate.
- Classify safe repairable drift, preserved maintainer differences, and dangerous ambiguity; duplicate markers, conflicting topology, missing repository, or manual closure without landed receipt fail closed.
- Derive lifecycle labels only from immutable Rev11 state. GitHub closure cannot satisfy predecessors, and mutable GitHub state cannot erase a finding carry-forward obligation.
- Prove second identical apply is a no-op and interrupted apply retries without duplication.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject title-based identity, full-body replacement, full-label replacement, milestone writes, issue-closure authority, and automatic repair of dangerous ambiguity.
- Do not create PRs, import CI evidence, inspect non-DAG issues, or schedule releases in this block.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 12 production files, 3 related packages.
- Rescope if reconciliation requires a second identity system or generic bidirectional synchronization.
- Correctness budget: zero human-field loss, duplicate issue, wrong blocker, authority inversion, or non-idempotent write.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on duplicate markers, incompatible parents, unrepresentable blocker topology, stale activation/authority, or any planned maintainer-owned mutation.
- Abort if one train/node cannot map deterministically without title inference.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Carry-forward obligation liveness comes from immutable receipts, never mutable GitHub issue closure, labels, or milestone movement.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH2-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:97-163`
- Applicability: `GH2`
- Exact text SHA-256: `529d0b1c1456691afdc20b0f9ddc69a6f206010f03f7f6cb452625d2008bd5da`

~~~~markdown
GH2 — DAG → GitHub issue projection and reconciliation
Predecessor: GH1

Project the static DAG onto GitHub:

* one train = one parent issue;
* one DAG node/block = one issue/sub-issue;
* nested DAG subblocks may become nested sub-issues;
* DAG predecessors = GitHub `blocked by` relationships;
* add issues to Project 3;
* apply structural/lifecycle labels;
* preserve all maintainer-owned metadata.

Use durable machine markers, not issue titles, to resolve identity, e.g.

```
<!-- verter:dag-train=compiler.compiler-bridge -->
<!-- verter:dag-node=CCA1 -->
```

Never use titles as identity.

Issue bodies must contain bounded managed regions. Human-authored content outside those regions must survive byte-for-byte.

Example:

```
<!-- verter:managed:start -->
...generated DAG metadata...
<!-- verter:managed:end -->

...human-maintained issue text...
```

Reconciliation must be idempotent.

A second identical `sync --apply` must produce no mutations.

Support:

```
githubctl sync --check
githubctl sync --apply
```

At minimum distinguish:

* safe repairable projection drift;
* maintainer-owned differences that must be preserved;
* dangerous/ambiguous drift that must fail closed.

Examples of dangerous drift:

* two GitHub issues claiming the same DAG node;
* one issue claiming two DAG nodes;
* a DAG node mapped to a missing repository;
* incompatible parent mappings;
* a DAG issue manually closed while no landed receipt exists;
* GitHub blocker topology that conflicts with generated DAG topology.

GitHub closure must NEVER make a DAG predecessor satisfied.

Only Rev11 receipts do that.

Do not overwrite milestones.

Milestones are maintainer-owned.
~~~~
