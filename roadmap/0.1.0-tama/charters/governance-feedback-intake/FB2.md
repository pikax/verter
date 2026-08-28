<!-- unified-charter-v2
id=FB2
name=Maintainer-authored DAG block and issue mapping
predecessors=FB1,GH2
phase=governance
train=governance.feedback-intake
product=feedback_intake
kind=contract
semantic_role=delivery
class=successor
owner=governance.feedback-intake:manual DAG authoring reusing the durable source issue
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
charter=charters/governance-feedback-intake/FB2.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FB2 — Maintainer-authored DAG block and issue mapping

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Ratify the manual-only path for turning an existing issue into planned DAG work. A maintainer authors the DAG node, charter, and `gh_issue` mapping in an ordinary reviewed patch. No command proposes, generates, imports, or applies DAG authority from GitHub, and this node does not implement the product work.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama/contracts/github-control-plane.md`, `roadmap/0.1.0-tama/authority/dag`, `roadmap/0.1.0-tama/charters`, `roadmap/0.1.0-tama/authority/state/implemented.toml`.
- Test surface: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`.
- Named boundaries: `ManualDagAuthoring`, reviewed DAG/charter patch, `GitHubIssueMapping` reuse, and source-issue provenance.

## Exact predecessor contracts

- **FB1:** implemented ledger row for “Non-DAG issue inspection”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **GH2:** implemented ledger row for “Occasional issue-sync command and local mapping”; ledger presence alone satisfies the predecessor and its commit metadata remains an unvalidated locator.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- The maintainer manually authors an ordinary reviewed patch containing the train, node, predecessors, charter, and any useful source-issue provenance.
- In that same manually authored patch, add the new node's `[[github_issue]]` row using the original issue number and `sync_to_github = false`; mapping presence does not mark the node implemented.
- `githubctl sync-issues` never creates or edits DAG/charter/ledger authority from GitHub and must never update this protected pre-existing issue.
- Preserve the issue number, comments, discussion, milestone, and unrelated labels. Add no DAG metadata, managed region, parent edge, blocker edge, or `dag:*` label.
- Refuse ambiguous or conflicting manual mappings. Issue closure cannot disposition P0/P1 or change implementation-ledger state.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FB2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **FB2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FB2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FB2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject automatic issue-to-DAG proposal/import, label-only DAG mutation, duplicate issue creation, DAG metadata/managed regions, and product implementation inside this mapping patch.
- Do not resolve carry-forward findings through DAG mapping or mutable GitHub state.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related packages.
- Rescope before adding any executable issue-to-DAG path or second authority database.
- Correctness budget: zero automatic authority mutation, duplicate mapping, wrong issue number, lost provenance, human-field loss, or finding erasure.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on a missing manually authored DAG/charter patch, a changed source issue number, a duplicate mapping, or inability to reuse the issue safely.
- Abort rather than synthesize authority from any GitHub field.

## Targeted verification

1. `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy.
