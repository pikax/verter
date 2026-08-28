<!-- unified-charter-v2
id=FB0
name=Non-DAG issue label state machine and feedback contract
predecessors=GH0
conditional_predecessors=
phase=governance
train=governance.feedback-intake
product=feedback_intake
kind=contract
semantic_role=delivery
class=successor
owner=governance.feedback-intake:namespaced AI label state and maintainer-owned feedback guards
conflict_domains=feedback_operations,github_projection_state
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
charter=charters/governance-feedback-intake/FB0.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FB0 — Non-DAG issue label state machine and feedback contract

Authority state is derived at dispatch. Contract ratification requires the current accepted ORC0 activation receipt and current GH0 constitution receipt.

## Independently acceptable outcome

Define one closed, namespaced non-DAG feedback label state machine and its ownership/security rules without implementing inspection. Pre-scope result: AI-result states, maintainer guards, DAG labels, and finding-carry-forward issue semantics form one contract boundary; issue inspection remains FB1.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11/contracts/github-control-plane.md`, `docs/arch/refactor/rev11/schemas/finding-carry-forward.schema.json`.
- Future implementation surfaces: `scripts/githubctl`, `.feedback/issues`.
- Named boundaries: `AiIssueVerdict`, `AiOwnedLabels`, `MaintainerGuards`, `DagLabels`, `FeedbackReport`, `FindingCarryForward`.

## Exact predecessor contracts

- **GH0:** exact current receipt ID and digest for “GitHub control-plane contract and authority matrix”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Define mutually exclusive AI results: unchecked, confirmed, rejected, fixed, and needs-human.
- Define `ai:ignore` and promotion authorization as maintainer-owned; AI cannot create/remove/override them.
- Define structural and lifecycle `dag:*` labels as projections of static DAG plus immutable receipts.
- Require inspection of existing labels before creation and reuse semantically equivalent vocabulary rather than duplicate namespaces.
- Bind durable issues used by finding carry-forward through stable repository/issue database identity; mutable GitHub labels, milestone, or closure cannot erase the obligation.
- Keep P0/P1 non-dispositionable and require escalating authorization for repeated lower-severity carry-forward.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FB0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **FB0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FB0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FB0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject `ai:checked` without distinct semantics, whole-label-set replacement, AI removal of `ai:ignore`, AI-created promotion approval, and issue closure as finding resolution.
- Do not inspect issues, write `.feedback` reports, or promote issues in this block.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related packages.
- Rescope before adding executable GitHub mutation or inspection logic.
- Correctness budget: zero label-owner crossing, ambiguous result state, implicit promotion, or lost finding.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if current repository vocabulary cannot be mapped without a maintainer policy decision, if ownership overlaps, or if immutable carry-forward cannot survive mutable issue changes.
- Abort on stale ORC0 or GH0 receipt.

## Targeted verification

1. `node --test docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Lower finding carry-forward stays live through immutable receipts regardless of mutable GitHub issue or label state.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-FB0-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:336-387`
- Applicability: `FB0`
- Exact text SHA-256: `ad2297f136dfbb5fed94a089bae962b8de33079d85c4e37c2ea84db123b82c55`

~~~~markdown
FB0 — Non-DAG issue label state machine and feedback contract
Predecessor: GH0

Design and implement a namespaced label contract.

Recommended shape:

AI result states, mutually exclusive:

```
ai:unchecked
ai:confirmed
ai:rejected
ai:fixed
ai:needs-human
```

Maintainer-controlled guards:

```
ai:ignore
dag:promotion-approved
```

DAG structure:

```
dag:train
dag:node
dag:release
```

DAG lifecycle projection:

```
dag:blocked
dag:ready
dag:leased
dag:review
dag:gate
dag:landed
```

Inspect current repository labels before creating anything.
Reuse a semantically equivalent existing label if one already exists.
Do not create duplicate vocabularies.

`ai:checked` is not required unless you find a concrete non-overlapping semantic meaning for it. Prefer `ai:needs-human` for an inspected but inconclusive issue.

`ai:ignore` is maintainer-owned. AI must never remove or override it.

`dag:promotion-approved` is maintainer-owned. AI must never create it itself.
~~~~
