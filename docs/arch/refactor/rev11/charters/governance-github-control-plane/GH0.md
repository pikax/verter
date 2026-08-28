<!-- unified-charter-v2
id=GH0
name=GitHub control-plane contract and authority matrix
predecessors=ORC0
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=constitution
semantic_role=delivery
class=successor
owner=governance.github-control-plane:field-owned GitHub projection over immutable Rev11 lifecycle authority
conflict_domains=github_projection_state,feedback_operations,release_orchestration
resource_class=docs-light
gate_profile=docs-domain
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
charter=charters/governance-github-control-plane/GH0.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH0 — GitHub control-plane contract and authority matrix

Authority state is derived at dispatch. No GitHub check or mutation may run until the current accepted ORC0 activation receipt proves the loaded authority is active and current.

## Independently acceptable outcome

Ratify one field-owned GitHub control-plane constitution without implementing synchronization. Pre-scope result: authority hierarchy, ownership matrix, identity markers, managed regions, finding retention, drift classes, security posture, and fixed `pikax/verter` plus Project `pikax/3` configuration form one atomic policy boundary. Runtime adapter code remains GH1.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11/contracts/github-control-plane.md`, `docs/arch/refactor/rev11/schemas/github-control-plane-program.schema.json`, `docs/arch/refactor/rev11/schemas/finding-carry-forward.schema.json`.
- Named boundaries: `GitHubControlPlaneContract`, `FieldOwner`, `DriftClass`, `FindingCarryForward`, `ImmutableResolutionReceipt`.
- Mutation boundary: authority and schema bytes only; no `gh`, network, issue, PR, Project, label, milestone, or workflow mutation.

## Exact predecessor contracts

- **ORC0:** exact current receipt ID and digest for “Orchestration v2 cutover and immutable-receipt migration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Encode the exact hierarchy: static DAG/contracts/charters, then immutable receipts, then leases, with GitHub as operational projection only.
- Assign every synchronized field exactly one of DAG, maintainer, GitHub-runtime, AI, or derived ownership.
- Define stable machine markers, one bounded managed region, owned label namespaces, dry-run/apply discipline, idempotence, and dangerous-drift refusal.
- Make the no-finding-loss invariant binding: P0/P1 block; lower actionable findings require policy-owned non-actionable acceptance or immutable receipt plus durable-issue carry-forward; resolution requires immutable supersession; mutable GitHub state cannot erase; repeated carry-forward escalates authorization.
- Define `landed_tree == validated_integration_tree`, orchestrator-only squash landing, milestone non-authority, and receipt-only predecessor satisfaction.
- Do not implement `scripts/githubctl` or inspect live repository configuration in this block.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject any proposal for generic bidirectional sync, title identity, full-label replacement, mutable GitHub acceptance, implicit repeated carry-forward, or GitHub closure satisfying predecessors.
- No legacy production path is deleted by this constitution; GH1–GH6, FB0–FB2, and REL0–REL2 own future implementation and cutover.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related packages.
- Rescope before adding executable adapter logic or live GitHub state.
- Correctness budget: zero ownership ambiguity, lost finding, mutable-state authority, hidden mutation, or stale activation acceptance.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if ORC0 is not landed/current, any field lacks one owner, any finding path lacks immutable liveness, or a policy requires GitHub state as correctness authority.
- Abort if the policy cannot be implemented by a replaceable structured adapter without scraping prose.

## Targeted verification

1. `node --test docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Apply `security-3`. Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports on the exact candidate. P0/P1 block. Every surviving actionable lower-severity finding follows the carry-forward contract; no mutable GitHub issue operation can dispose it.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH0-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:47-61`
- Applicability: `GH0`
- Exact text SHA-256: `5b9cbdc1973a32cd25f335c1d9af76dfe056e3e31eb7250c6cd626092c8ee36f`

~~~~markdown
GH0 — GitHub control-plane contract and authority matrix
Predecessor: ORC0

Define:

* authority ownership for every synchronized datum;
* GitHub structural markers;
* label namespaces;
* managed-body boundaries;
* drift classes;
* reconciliation rules;
* security/fail-closed rules;
* configuration for `pikax/verter` and Project `pikax/3`.

No live synchronization implementation yet.
~~~~

### SRC-GITHUB-PROGRAM-GOAL

- Kind: `context`
- Source: `github-control-plane-program.md:1-45`
- Applicability: `GH0`, `GH1`, `GH2`, `GH3`, `GH4`, `GH5`, `GH6`, `FB0`, `FB1`, `FB2`, `REL0`, `REL1`, `REL2`
- Exact text SHA-256: `171f0f26830374bf948968acfe28e8eab792f78c3fcb1c5ed3de05896b76b534`

~~~~markdown
Implement a new post-ORC0 GitHub control-plane program for Rev11.

Repository:
https://github.com/pikax/verter

Rev11:
docs/arch/refactor/rev11

Existing GitHub Project:
https://github.com/users/pikax/projects/3/views/1
Owner: pikax
Project number: 3

This work MUST begin only after ORC0 has landed and its accepted receipt is current.

# Goal

Move Rev11 operational orchestration onto GitHub Issues, sub-issues, PRs, CI and milestones while preserving the architectural guarantees established by ORC0.

GitHub is to become the operational control plane and historical UI.

It MUST NOT replace Rev11's correctness authority.

The final ownership model must remain:

* static Rev11 DAG/charters/contracts = architecture authority;
* immutable `programctl` receipts = lifecycle/correctness authority;
* ephemeral leases = active work ownership;
* GitHub Issues/Projects/PRs = operational projection, coordination and history;
* GitHub CI = gate executor whose exact evidence is imported/bound into the Rev11 evidence model;
* milestones = maintainer-owned release-planning metadata.

Do not create a generic bidirectional synchronization system where either side can overwrite the other.

Implement field-owned reconciliation: every synchronized field must have exactly one authoritative owner.

# Mandatory architecture

Do not modify generated `program-dag.toml` directly.

Add proper authority DAG modules/charters/contracts using the post-ORC0 amendment mechanism.

Use these blocks unless current source proves a materially better decomposition. Do not silently combine them into larger blocks.

## governance.github-control-plane
~~~~

### SRC-GITHUB-OWNERSHIP-MATRIX

- Kind: `requirement`
- Source: `github-control-plane-program.md:653-697`
- Applicability: `GH0`, `GH2`, `FB0`, `REL0`
- Exact text SHA-256: `a8779a3851d606932c0726955ecdba769ab1ab5217c047f37a06ec0032618cba`

~~~~markdown
# Synchronization ownership matrix

Implement this concept explicitly in code/contracts rather than leaving it implicit.

DAG-owned:

* node ID;
* train;
* charter-derived managed metadata;
* parent train relationship;
* DAG predecessor relationships;
* DAG lifecycle state;
* accepted/landed status.

GitHub-maintainer-owned:

* milestone;
* human issue text;
* human labels outside managed namespaces;
* `ai:ignore`;
* promotion authorization;
* manual release-cut authorization.

GitHub/runtime-owned:

* issue number;
* PR number;
* PR URL;
* draft/ready state;
* CI run/check identity;
* merge result.

AI-owned:

* AI triage result labels;
* `.feedback/issues/<id>.md`;
* managed PR summary/review-cycle presentation.

Derived/non-authoritative:

* GitHub Project Status;
* project grouping/views;
* convenience progress indicators.

Never permit derived GitHub Project state to become a correctness input.
~~~~

### SRC-GITHUB-PRESCOPE

- Kind: `requirement`
- Source: `github-control-plane-program.md:783-817`
- Applicability: `GH0`, `GH1`, `GH2`, `GH3`, `GH4`, `GH5`, `GH6`, `FB0`, `FB1`, `FB2`, `REL0`, `REL1`, `REL2`
- Exact text SHA-256: `31145063dbbd69d54fe57bd2c5f02656b653b4ce0ec7473773e8fac9164369b3`

~~~~markdown
# Existing surfaces to inspect before implementation

At minimum inspect the post-ORC0 versions of:

* `docs/arch/refactor/rev11/authority/**`
* `docs/arch/refactor/rev11/contracts/**`
* `docs/arch/refactor/rev11/tools/programctl.mjs`
* `docs/arch/refactor/rev11/tools/lib.mjs`
* `docs/arch/refactor/rev11/tools/trusted-local.mjs`
* amendment tooling
* lifecycle tests
* evidence schemas
* `.github/workflows/ci.yml`
* `.github/workflows/release-check.yml`
* `.github/workflows/release-tag.yml`
* `.github/workflows/release.yml`
* repository branch/ruleset/merge configuration
* Project 3 fields/items/views
* existing labels/milestones/issues.

Do not assume the current `codex/orc0-trusted-local` implementation is still identical after ORC0 lands. Re-inspect the landed `main` source before authoring the amendment.

# Scoping requirement

Before implementation, run the architect/pre-scope step on each proposed block.

If any block contains multiple independently acceptable outcomes, split it before dispatch.

Do not turn this proposal into another oversized train disguised as one block.

Breaking changes to orchestration tooling are allowed where they materially improve the architecture.

Prefer deletion/replacement over maintaining dual legacy/GitHub orchestration systems.

Do not create a permanent compatibility layer.
~~~~

### SRC-GITHUB-FINDING-RETENTION

- Kind: `requirement`
- Source: `github-control-plane-program.md:893-895`
- Applicability: `GH0`, `GH1`, `GH2`, `GH3`, `GH4`, `GH5`, `GH6`, `FB0`, `FB1`, `FB2`, `REL0`, `REL1`, `REL2`
- Exact text SHA-256: `7bc0237c2c5f045ecd2510049bad8901064e0de8fefbb6638e2625b9c2c59b98`

~~~~markdown
# Binding finding-retention invariant

No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.
~~~~
