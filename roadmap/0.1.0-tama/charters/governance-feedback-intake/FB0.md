<!-- unified-charter-v2
id=FB0
name=Non-DAG issue label state machine and feedback contract
predecessors=GH0
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
external_requirements=
charter=charters/governance-feedback-intake/FB0.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FB0 — Non-DAG issue label state machine and feedback contract

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Define one closed, namespaced non-DAG feedback label state machine and its ownership/security rules without implementing inspection. AI-result states, maintainer guards, and finding-carry-forward issue semantics form one contract boundary; issue inspection remains FB1. No label carries DAG identity, topology, readiness, or implementation state.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama/contracts/github-control-plane.md`, `roadmap/0.1.0-tama/schemas/finding-carry-forward.schema.json`.
- Future implementation surfaces: `scripts/githubctl`, `.feedback/issues`.
- Named boundaries: `AiIssueVerdict`, `AiOwnedLabels`, `MaintainerGuards`, `FeedbackReport`, and `FindingCarryForward`.

## Exact predecessor contracts

- **GH0:** implemented ledger row for “Minimal GitHub workflow and local issue mapping”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Define mutually exclusive AI results: unchecked, confirmed, rejected, fixed, and needs-human.
- Define `ai:ignore` as maintainer-owned; AI cannot create, remove, or override it. Promotion authorization is an explicit maintainer action and is never inferred from an issue label.
- Forbid structural or lifecycle `dag:*` labels and any GitHub-side projection of DAG identity, topology, readiness, or implementation rows.
- Require inspection of existing AI-result labels before creation and reuse semantically equivalent AI vocabulary rather than duplicate namespaces.
- Use durable issue URLs or database IDs when the owning review policy calls for lower-severity follow-up.
- Keep P0/P1 blocking; lower-severity findings follow the owning review policy.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FB0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **FB0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FB0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FB0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject `ai:checked` without distinct semantics, whole-label-set replacement, AI removal of `ai:ignore`, AI-created promotion approval, any `dag:*` label, and issue closure as finding resolution.
- Do not inspect issues, write `.feedback` reports, or author DAG blocks in this block.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related packages.
- Rescope before adding executable GitHub mutation or inspection logic.
- Correctness budget: zero label-owner crossing, ambiguous result state, implicit DAG mutation, or lost finding.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if current repository vocabulary cannot be mapped without a maintainer policy decision or if ownership overlaps.
- Abort if ORC0 or GH0 lacks an implementation-ledger row.

## Targeted verification

1. `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy. Ledger rows record node completion, not findings.
