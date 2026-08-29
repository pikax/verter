<!-- unified-charter-v2
id=GH5
name=CI, ledger finalization and squash landing
predecessors=GH4
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:GitHub CI and reviewed PR squash-merge landing
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
external_requirements=
charter=charters/governance-github-control-plane/GH5.md
size=M
max_production_loc=1200
max_production_files=12
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH5 — CI, ledger finalization and squash landing

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


## Independently acceptable outcome

Use GitHub CI for integration verification, let the finishing agent complete the implementation row with final-title message, approximate timezone-bearing date, and PR number, then land by squash-merging the reviewed PR through GitHub.

## Concrete surfaces and APIs

- Production surfaces: `.github`, `scripts/githubctl`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: `CiResult`, reviewed GitHub-PR squash merge, and pre-review implementation-row finalization; local-first landing/mirroring is forbidden.

## Exact predecessor contracts

- **GH4:** implemented ledger row for “Human review history and model attribution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Present CI commands, skips, and terminal results as review evidence without binding them to a commit SHA or tree.
- Missing jobs or unexpected skips fail the owning gate; agents rerun affected checks after material changes.
- At the end of implementation and before squash/final review, update the same-patch `[[implemented]]` row with the expected final title as `commit_message`, approximate date with timezone, and known `pull_request`.
- Squash-merge the reviewed PR through GitHub after review and gates pass; never land the candidate locally first and mirror it afterward.
- The reviewed patch already contains the ledger row. No landing receipt, post-merge ledger update, or Git identity comparison follows.
- P0/P1 block. Lower findings follow the owning review policy.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH5-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject candidate/landed SHA invariants, landing receipts, automatic close keywords as correctness, stale check reuse, and post-merge state advancement.
- Do not create a second evidence or lifecycle database around GitHub checks.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 12 production files, 3 related packages.
- Rescope if a second evidence or identity system is proposed.
- Correctness budget: zero skipped evidence, unauthorized merge, lost finding, missing PR locator at completion when known, or ledger state inferred from GitHub.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on a missing ledger predecessor, incomplete review/finding disposition, missing CI job, or unexpected skip.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy. Ledger rows record node completion, not findings.
